// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::collections::HashMap;
use std::io::Write;

use chrono::NaiveDate;
use reqwest::StatusCode;

use crate::cache;
use crate::models::crash_pings::{
    CrashPingFilters, CrashPingFrame, CrashPingStackResponse, CrashPingStackSummary,
    CrashPingsItem, CrashPingsResponse, CrashPingsSummary,
};
use crate::output::{OutputFormat, compact, json, markdown};
use crate::{Error, PREVIEW_BYTES, Result, status_error, truncate_on_char_boundary};

const BASE_URL: &str = "https://crash-pings.mozilla.org";

/// Fetches one day of crash pings, from the local cache if it is there.
///
/// Status mapping: `200` parses and is cached; `202` (the day's data is not
/// built yet) becomes an explanatory [`Error::ParseError`]; `404` becomes
/// [`Error::NotFound`]; anything else goes through [`status_error`], so a 4xx
/// or 5xx is an [`Error::Http`] and any other status is an
/// [`Error::UnexpectedStatus`].
fn fetch_ping_data(
    client: &reqwest::blocking::Client,
    base_url: &str,
    date: &str,
) -> Result<CrashPingsResponse> {
    let cache_key = format!("crash-pings-{}.json", date);

    // Try cache first
    if let Some(cached) = cache::read_cached(&cache_key) {
        let resp: CrashPingsResponse = serde_json::from_slice(&cached)
            .map_err(|e| Error::ParseError(format!("cached data parse error: {}", e)))?;
        return Ok(resp);
    }

    let url = format!("{}/ping_data/{}", base_url, date);
    let response = client.get(&url).send()?;

    match response.status() {
        StatusCode::OK => {
            let bytes = response.bytes()?;
            // Cache the raw response
            cache::write_cache(&cache_key, &bytes);
            serde_json::from_slice(&bytes).map_err(|e| {
                Error::ParseError(format!(
                    "{}: {}",
                    e,
                    // The shared `PREVIEW_BYTES` cap, but applied to the raw
                    // bytes: slicing a byte slice cannot panic on a character
                    // boundary, so this preview needs no
                    // `truncate_on_char_boundary`; `from_utf8_lossy` repairs
                    // whatever sequence the cap happens to split.
                    String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_BYTES)])
                ))
            })
        }
        StatusCode::ACCEPTED => Err(Error::ParseError(format!(
            "Crash ping data for {} is not available (HTTP 202). \
                 Today's data typically appears around 04:00 UTC. \
                 Older dates may also be unavailable.",
            date
        ))),
        StatusCode::NOT_FOUND => Err(Error::NotFound(format!(
            "No crash ping data for date {}. Data is available from September 2024 onwards.",
            date
        ))),
        _ => Err(status_error(response)),
    }
}

/// Fetches the stack for a single crash ping. Not cached.
///
/// Status mapping: `200` parses; `404` becomes [`Error::NotFound`]; anything
/// else goes through [`status_error`], which keeps [`Error::Http`] for a
/// genuine 4xx/5xx and reports any other status -- notably the `202` this
/// endpoint's sibling serves -- as [`Error::UnexpectedStatus`].
fn fetch_stack(
    client: &reqwest::blocking::Client,
    base_url: &str,
    date: &str,
    crash_id: &str,
) -> Result<CrashPingStackResponse> {
    let url = format!("{}/stack/{}/{}", base_url, date, crash_id);
    let response = client.get(&url).send()?;

    match response.status() {
        StatusCode::OK => {
            let text = response.text()?;
            serde_json::from_str(&text).map_err(|e| {
                Error::ParseError(format!(
                    "{}: {}",
                    e,
                    truncate_on_char_boundary(&text, PREVIEW_BYTES)
                ))
            })
        }
        StatusCode::NOT_FOUND => Err(Error::NotFound(format!(
            "Stack not found for crash ping {} on {}",
            crash_id, date
        ))),
        _ => Err(status_error(response)),
    }
}

fn date_range(from: &str, to: &str) -> Vec<String> {
    let start = NaiveDate::parse_from_str(from, "%Y-%m-%d").expect("invalid start date");
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").expect("invalid end date");
    let mut dates = Vec::new();
    let mut current = start;
    while current <= end {
        dates.push(current.format("%Y-%m-%d").to_string());
        current += chrono::Duration::days(1);
    }
    dates
}

fn aggregate(
    responses: &[&CrashPingsResponse],
    filters: &CrashPingFilters,
    facet: &str,
    limit: usize,
    date_from: &str,
    date_to: &str,
) -> CrashPingsSummary {
    let mut counts: HashMap<String, (usize, Vec<String>)> = HashMap::new();
    let mut total = 0usize;
    let mut filtered_total = 0usize;

    for response in responses {
        total += response.len();
        for i in 0..response.len() {
            if !response.matches_filters(i, filters) {
                continue;
            }
            filtered_total += 1;
            let value = response.facet_value(i, facet);
            let entry = counts.entry(value).or_insert_with(|| (0, Vec::new()));
            entry.0 += 1;
            if entry.1.len() < 3 {
                entry.1.push(response.crashid[i].clone());
            }
        }
    }

    let mut items: Vec<(String, usize, Vec<String>)> = counts
        .into_iter()
        .map(|(k, (count, ids))| (k, count, ids))
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.1));
    items.truncate(limit);

    let items = items
        .into_iter()
        .map(|(label, count, example_ids)| {
            let percentage = if filtered_total > 0 {
                count as f64 / filtered_total as f64 * 100.0
            } else {
                0.0
            };
            CrashPingsItem {
                label,
                count,
                percentage,
                example_ids,
            }
        })
        .collect();

    CrashPingsSummary {
        date_from: date_from.to_string(),
        date_to: date_to.to_string(),
        total,
        filtered_total,
        signature_filter: filters.signature.clone(),
        facet_name: facet.to_string(),
        items,
    }
}

pub fn execute(
    date_from: &str,
    date_to: &str,
    filters: CrashPingFilters,
    facet: &str,
    limit: usize,
    stack_id: Option<&str>,
    format: OutputFormat,
) -> Result<()> {
    let client = reqwest::blocking::Client::builder().gzip(true).build()?;

    const VALID_FACETS: &[&str] = &[
        "signature",
        "channel",
        "os",
        "process",
        "version",
        "arch",
        "osversion",
        "build_id",
        "ipc_actor",
        "reason",
        "type",
    ];
    if !VALID_FACETS.contains(&facet) {
        return Err(Error::ParseError(format!(
            "Unknown facet \"{}\". Valid facets: {}",
            facet,
            VALID_FACETS.join(", ")
        )));
    }

    if let Some(crash_id) = stack_id {
        // Stack mode (date_from == date_to since --stack conflicts with range args)
        let resp = fetch_stack(&client, BASE_URL, date_from, crash_id)?;
        let frames = resp.stack.unwrap_or_default();
        let summary = CrashPingStackSummary {
            crash_id: crash_id.to_string(),
            date: date_from.to_string(),
            frames,
            java_exception: resp.java_exception,
        };
        let output = match format {
            OutputFormat::Compact => compact::format_crash_ping_stack(&summary),
            OutputFormat::Json => json::format_crash_ping_stack(&summary)?,
            OutputFormat::Markdown => markdown::format_crash_ping_stack(&summary),
        };
        print!("{}", output);
    } else {
        // Aggregate mode
        let dates = date_range(date_from, date_to);
        let multi_date = dates.len() > 1;
        let mut responses = Vec::new();

        for (idx, date) in dates.iter().enumerate() {
            if multi_date {
                eprint!("\rFetching crash pings: {}/{}...", idx + 1, dates.len());
                std::io::stderr().flush().ok();
            }
            match fetch_ping_data(&client, BASE_URL, date) {
                Ok(resp) => responses.push(resp),
                Err(Error::NotFound(_)) | Err(Error::ParseError(_)) => {
                    // 404 or 202 — skip with warning
                    eprintln!("\rWarning: no data for {}, skipping.          ", date);
                }
                Err(e) => return Err(e),
            }
        }

        if multi_date {
            // Clear the progress line
            eprint!("\r                                              \r");
            std::io::stderr().flush().ok();
        }

        let response_refs: Vec<&CrashPingsResponse> = responses.iter().collect();
        let summary = aggregate(&response_refs, &filters, facet, limit, date_from, date_to);
        let output = match format {
            OutputFormat::Compact => compact::format_crash_pings(&summary),
            OutputFormat::Json => json::format_crash_pings(&summary)?,
            OutputFormat::Markdown => markdown::format_crash_pings(&summary),
        };
        print!("{}", output);
    }

    Ok(())
}

fn format_frame(frame: &CrashPingFrame) -> String {
    if let Some(func) = &frame.function {
        func.clone()
    } else if let Some(offset) = &frame.offset {
        if let Some(module) = &frame.module {
            format!("{} ({})", offset, module)
        } else {
            offset.clone()
        }
    } else {
        "???".to_string()
    }
}

pub fn format_frame_location(frame: &CrashPingFrame) -> String {
    let func = format_frame(frame);
    match (&frame.file, frame.line) {
        (Some(file), Some(line)) => format!("{} @ {}:{}", func, file, line),
        (Some(file), None) => format!("{} @ {}", func, file),
        _ => func,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The one copy of the cache-redirect guard, shared with `cache`'s own
    // tests. Every `fetch_ping_data` test needs it: cache keys carry no
    // base-URL component and the cache is consulted before the request URL is
    // built, so a test pointed at the local server would otherwise hit the
    // user's real production cache.
    use crate::cache::RedirectedCache;
    use crate::test_server::TestServer;
    use serde_json::json;
    use serial_test::serial;

    fn make_test_response() -> CrashPingsResponse {
        let data = json!({
            "channel": {
                "strings": ["release", "beta"],
                "values": [0, 0, 1, 0, 0]
            },
            "process": {
                "strings": ["main", "content"],
                "values": [0, 1, 0, 1, 0]
            },
            "ipc_actor": {
                "strings": [null],
                "values": [0, 0, 0, 0, 0]
            },
            "clientid": {
                "strings": ["c1", "c2", "c3", "c4", "c5"],
                "values": [0, 1, 2, 3, 4]
            },
            "crashid": ["id1", "id2", "id3", "id4", "id5"],
            "version": {
                "strings": ["147.0"],
                "values": [0, 0, 0, 0, 0]
            },
            "os": {
                "strings": ["Windows", "Linux"],
                "values": [0, 0, 1, 0, 1]
            },
            "osversion": {
                "strings": ["10.0"],
                "values": [0, 0, 0, 0, 0]
            },
            "arch": {
                "strings": ["x86_64"],
                "values": [0, 0, 0, 0, 0]
            },
            "date": {
                "strings": ["2026-02-12"],
                "values": [0, 0, 0, 0, 0]
            },
            "reason": {
                "strings": [null],
                "values": [0, 0, 0, 0, 0]
            },
            "type": {
                "strings": [null],
                "values": [0, 0, 0, 0, 0]
            },
            "minidump_sha256_hash": [null, null, null, null, null],
            "startup_crash": [false, false, false, false, false],
            "build_id": {
                "strings": ["20260210"],
                "values": [0, 0, 0, 0, 0]
            },
            "signature": {
                "strings": ["OOM | small", "setup_stack_prot"],
                "values": [0, 0, 0, 1, 1]
            }
        });
        serde_json::from_value(data).unwrap()
    }

    #[test]
    fn test_aggregate_by_signature() {
        let resp = make_test_response();
        let filters = CrashPingFilters::default();
        let summary = aggregate(
            &[&resp],
            &filters,
            "signature",
            10,
            "2026-02-12",
            "2026-02-12",
        );
        assert_eq!(summary.total, 5);
        assert_eq!(summary.filtered_total, 5);
        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[0].label, "OOM | small");
        assert_eq!(summary.items[0].count, 3);
        assert_eq!(summary.items[0].example_ids.len(), 3);
        assert_eq!(summary.items[0].example_ids, vec!["id1", "id2", "id3"]);
        assert_eq!(summary.items[1].label, "setup_stack_prot");
        assert_eq!(summary.items[1].count, 2);
        assert_eq!(summary.items[1].example_ids.len(), 2);
        assert_eq!(summary.items[1].example_ids, vec!["id4", "id5"]);
    }

    #[test]
    fn test_aggregate_with_filter() {
        let resp = make_test_response();
        let filters = CrashPingFilters {
            os: Some("Windows".to_string()),
            ..Default::default()
        };
        let summary = aggregate(
            &[&resp],
            &filters,
            "signature",
            10,
            "2026-02-12",
            "2026-02-12",
        );
        assert_eq!(summary.filtered_total, 3);
        // Only Windows pings: id1, id2, id4
        assert_eq!(summary.items[0].example_ids, vec!["id1", "id2"]);
    }

    #[test]
    fn test_aggregate_by_os() {
        let resp = make_test_response();
        let filters = CrashPingFilters::default();
        let summary = aggregate(&[&resp], &filters, "os", 10, "2026-02-12", "2026-02-12");
        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[0].label, "Windows");
        assert_eq!(summary.items[0].count, 3);
        assert_eq!(summary.items[0].example_ids, vec!["id1", "id2", "id4"]);
        assert_eq!(summary.items[1].label, "Linux");
        assert_eq!(summary.items[1].count, 2);
        assert_eq!(summary.items[1].example_ids, vec!["id3", "id5"]);
    }

    #[test]
    fn test_aggregate_limit() {
        let resp = make_test_response();
        let filters = CrashPingFilters::default();
        let summary = aggregate(
            &[&resp],
            &filters,
            "signature",
            1,
            "2026-02-12",
            "2026-02-12",
        );
        assert_eq!(summary.items.len(), 1);
        assert_eq!(summary.items[0].label, "OOM | small");
        assert_eq!(summary.items[0].example_ids.len(), 3);
    }

    #[test]
    fn test_aggregate_percentages() {
        let resp = make_test_response();
        let filters = CrashPingFilters::default();
        let summary = aggregate(
            &[&resp],
            &filters,
            "signature",
            10,
            "2026-02-12",
            "2026-02-12",
        );
        assert!((summary.items[0].percentage - 60.0).abs() < 0.01);
        assert!((summary.items[1].percentage - 40.0).abs() < 0.01);
        assert!(!summary.items[0].example_ids.is_empty());
    }

    #[test]
    fn test_aggregate_multi_response() {
        let resp1 = make_test_response();
        let resp2 = make_test_response();
        let filters = CrashPingFilters::default();
        let summary = aggregate(
            &[&resp1, &resp2],
            &filters,
            "signature",
            10,
            "2026-02-12",
            "2026-02-13",
        );
        assert_eq!(summary.total, 10);
        assert_eq!(summary.filtered_total, 10);
        assert_eq!(summary.items[0].label, "OOM | small");
        assert_eq!(summary.items[0].count, 6);
        // Capped at 3 example IDs even with 6 matching pings
        assert_eq!(summary.items[0].example_ids.len(), 3);
        assert_eq!(summary.items[1].label, "setup_stack_prot");
        assert_eq!(summary.items[1].count, 4);
        assert_eq!(summary.items[1].example_ids.len(), 3);
        assert_eq!(summary.date_from, "2026-02-12");
        assert_eq!(summary.date_to, "2026-02-13");
    }

    #[test]
    fn test_date_range() {
        let dates = date_range("2026-02-10", "2026-02-13");
        assert_eq!(
            dates,
            vec!["2026-02-10", "2026-02-11", "2026-02-12", "2026-02-13"]
        );
    }

    #[test]
    fn test_date_range_single_day() {
        let dates = date_range("2026-02-10", "2026-02-10");
        assert_eq!(dates, vec!["2026-02-10"]);
    }

    #[test]
    fn test_format_frame_with_function() {
        let frame = CrashPingFrame {
            function: Some("mozilla::SomeFunc".to_string()),
            function_offset: None,
            file: None,
            line: None,
            module: None,
            module_offset: None,
            offset: None,
            omitted: None,
            error: None,
        };
        assert_eq!(format_frame(&frame), "mozilla::SomeFunc");
    }

    #[test]
    fn test_format_frame_with_offset_and_module() {
        let frame = CrashPingFrame {
            function: None,
            function_offset: None,
            file: None,
            line: None,
            module: Some("xul.dll".to_string()),
            module_offset: None,
            offset: Some("0x1234".to_string()),
            omitted: None,
            error: None,
        };
        assert_eq!(format_frame(&frame), "0x1234 (xul.dll)");
    }

    #[test]
    fn test_format_frame_unknown() {
        let frame = CrashPingFrame {
            function: None,
            function_offset: None,
            file: None,
            line: None,
            module: None,
            module_offset: None,
            offset: None,
            omitted: None,
            error: None,
        };
        assert_eq!(format_frame(&frame), "???");
    }

    #[test]
    fn test_format_frame_location_with_file() {
        let frame = CrashPingFrame {
            function: Some("EnsureTimeStretcher".to_string()),
            function_offset: None,
            file: Some("AudioDecoderInputTrack.cpp".to_string()),
            line: Some(624),
            module: None,
            module_offset: None,
            offset: None,
            omitted: None,
            error: None,
        };
        assert_eq!(
            format_frame_location(&frame),
            "EnsureTimeStretcher @ AudioDecoderInputTrack.cpp:624"
        );
    }

    /// A blocking client shaped like the one `execute` builds, but immune to
    /// ambient proxy configuration: the test server is on loopback, and an
    /// `http_proxy` in the environment would otherwise send the request
    /// somewhere else entirely. `gzip(true)` mirrors production; the test
    /// server sends no `Content-Encoding`, so plain bodies still decode.
    fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .gzip(true)
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("build blocking test client")
    }

    /// The `ParseError` preview must survive a body whose 200th byte lands
    /// inside a multi-byte character. `fetch_stack` used to build it by
    /// slicing the body to a flat 200-byte cap, which panics on exactly this
    /// input -- aborting the process at the moment a diagnostic was wanted, on
    /// a body that is arbitrary bytes off the network.
    #[test]
    fn fetch_stack_previews_a_body_split_mid_utf8_sequence_without_panicking() {
        // 199 ASCII bytes then a 3-byte em dash.
        let body = format!("{}\u{2014}", "a".repeat(199));
        assert_eq!(body.len(), 202);
        assert!(!body.is_char_boundary(200));

        let server = TestServer::start();
        server.push_response(200, body);

        let err = fetch_stack(&test_client(), &server.base_url(), "2026-09-01", "abc")
            .expect_err("a non-JSON body must not parse");

        let Error::ParseError(message) = &err else {
            panic!("expected Error::ParseError, got {err:?}");
        };
        // Truncated at the boundary *before* the em dash: byte 199 is a
        // boundary, byte 200 is not, so the preview is the 199 ASCII bytes.
        assert!(
            message.contains(&"a".repeat(199)),
            "preview lost the ASCII prefix: {message}"
        );
        assert!(
            !message.contains('\u{2014}'),
            "preview should stop before the split character: {message}"
        );
    }

    /// A one-ping `/ping_data/<date>` body: the smallest thing that
    /// deserializes into a `CrashPingsResponse`, so a test can prove the happy
    /// path still parses without carrying a multi-megabyte fixture.
    fn minimal_ping_data_body() -> String {
        json!({
            "channel": { "strings": ["release"], "values": [0] },
            "process": { "strings": ["main"], "values": [0] },
            "ipc_actor": { "strings": [null], "values": [0] },
            "clientid": { "strings": ["c1"], "values": [0] },
            "crashid": ["ping-id-1"],
            "version": { "strings": ["147.0"], "values": [0] },
            "os": { "strings": ["Windows"], "values": [0] },
            "osversion": { "strings": ["10.0"], "values": [0] },
            "arch": { "strings": ["amd64"], "values": [0] },
            "date": { "strings": ["2026-09-01"], "values": [0] },
            "reason": { "strings": [null], "values": [0] },
            "type": { "strings": [null], "values": [0] },
            "minidump_sha256_hash": [null],
            "startup_crash": [null],
            "build_id": { "strings": ["20260901000000"], "values": [0] },
            "signature": { "strings": ["OOM | small"], "values": [0] },
        })
        .to_string()
    }

    // --- Status mapping: the `_` fallthrough arms ---

    /// `crash-pings.mozilla.org` serves 202 while a day's data is still being
    /// built, and `reqwest`'s `error_for_status` treats 202 as success -- so
    /// the old fallthrough arm, which took the error out of that method's
    /// `Result` unconditionally, unwrapped an `Ok` and panicked.
    /// `fetch_stack` does not special-case 202, so it is the reachable one.
    #[test]
    fn fetch_stack_maps_202_to_unexpected_status_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let err = fetch_stack(&test_client(), &server.base_url(), "2026-09-01", "abc")
            .expect_err("202 is not a usable stack response");

        let Error::UnexpectedStatus { status, url } = &err else {
            panic!("expected Error::UnexpectedStatus, got {err:?}");
        };
        assert_eq!(*status, 202);
        assert_eq!(url, &format!("{}/stack/2026-09-01/abc", server.base_url()));
        assert_eq!(
            err.to_string(),
            format!(
                "Unexpected HTTP status 202 from {}/stack/2026-09-01/abc",
                server.base_url()
            )
        );
    }

    /// The ping-data fetch handles 202 itself, so its fallthrough is reached by
    /// some *other* non-error status. 204 stands in for that class.
    #[test]
    #[serial]
    fn fetch_ping_data_maps_204_to_unexpected_status_without_panicking() {
        let _cache = RedirectedCache::new();
        let server = TestServer::start();
        server.push_response(204, "");

        let err = fetch_ping_data(&test_client(), &server.base_url(), "2026-09-01")
            .expect_err("204 carries no ping data");

        let Error::UnexpectedStatus { status, url } = &err else {
            panic!("expected Error::UnexpectedStatus, got {err:?}");
        };
        assert_eq!(*status, 204);
        assert_eq!(url, &format!("{}/ping_data/2026-09-01", server.base_url()));
    }

    /// A genuine server error keeps `Error::Http`, which carries `reqwest`'s
    /// richer error, rather than being flattened into `UnexpectedStatus`.
    #[test]
    fn fetch_stack_maps_500_to_an_http_error() {
        let server = TestServer::start();
        server.push_response(500, "boom");

        let err = fetch_stack(&test_client(), &server.base_url(), "2026-09-01", "abc")
            .expect_err("500 is an error");

        let Error::Http(source) = &err else {
            panic!("expected Error::Http, got {err:?}");
        };
        assert_eq!(source.status().map(|s| s.as_u16()), Some(500));
    }

    #[test]
    #[serial]
    fn fetch_ping_data_maps_500_to_an_http_error() {
        let _cache = RedirectedCache::new();
        let server = TestServer::start();
        server.push_response(500, "boom");

        let err = fetch_ping_data(&test_client(), &server.base_url(), "2026-09-01")
            .expect_err("500 is an error");

        let Error::Http(source) = &err else {
            panic!("expected Error::Http, got {err:?}");
        };
        assert_eq!(source.status().map(|s| s.as_u16()), Some(500));
    }

    // --- Status mapping: the explicit arms, which must not regress ---

    /// The live server does this every day until roughly 04:00 UTC, and the
    /// explanatory message is the whole point of handling 202 here. It must not
    /// be rerouted to the generic `UnexpectedStatus`.
    #[test]
    #[serial]
    fn fetch_ping_data_maps_202_to_an_explanatory_parse_error() {
        let _cache = RedirectedCache::new();
        let server = TestServer::start();
        server.push_response(202, "");

        let err = fetch_ping_data(&test_client(), &server.base_url(), "2026-09-02")
            .expect_err("202 means the data is not built yet");

        let Error::ParseError(message) = &err else {
            panic!("expected Error::ParseError, got {err:?}");
        };
        assert!(message.contains("2026-09-02"), "{message}");
        assert!(message.contains("not available (HTTP 202)"), "{message}");
        assert!(message.contains("around 04:00 UTC"), "{message}");
    }

    #[test]
    #[serial]
    fn fetch_ping_data_maps_404_to_not_found() {
        let _cache = RedirectedCache::new();
        let server = TestServer::start();
        server.push_response(404, "");

        let err = fetch_ping_data(&test_client(), &server.base_url(), "2019-01-01")
            .expect_err("404 means no data for that date");

        let Error::NotFound(message) = &err else {
            panic!("expected Error::NotFound, got {err:?}");
        };
        assert!(message.contains("2019-01-01"), "{message}");
        assert!(
            message.contains("Data is available from September 2024 onwards"),
            "{message}"
        );
    }

    #[test]
    fn fetch_stack_maps_404_to_not_found() {
        let server = TestServer::start();
        server.push_response(404, "");

        let err = fetch_stack(
            &test_client(),
            &server.base_url(),
            "2026-09-01",
            "ping-id-1",
        )
        .expect_err("404 means no stack for that ping");

        let Error::NotFound(message) = &err else {
            panic!("expected Error::NotFound, got {err:?}");
        };
        assert!(
            message.contains("Stack not found for crash ping"),
            "{message}"
        );
        assert!(message.contains("ping-id-1"), "{message}");
        assert!(message.contains("2026-09-01"), "{message}");
    }

    // --- Happy paths, the cache, and the request URLs ---

    /// Proves three things at once: a 200 body still deserializes, the response
    /// is written to the cache, and the second call is served from that cache
    /// rather than the network -- only one response is queued, so a second
    /// request would get the harness's loud 500 and fail this test.
    #[test]
    #[serial]
    fn fetch_ping_data_parses_and_caches_a_successful_response() {
        let cache = RedirectedCache::new();
        let server = TestServer::start();
        server.push_response(200, minimal_ping_data_body());

        let first = fetch_ping_data(&test_client(), &server.base_url(), "2026-09-01")
            .expect("a well-formed body must parse");
        assert_eq!(first.len(), 1);
        assert_eq!(first.signature(0), "OOM | small");

        let cached = cache.path().join("crash-pings-2026-09-01.json");
        assert!(cached.is_file(), "response was not written to the cache");

        let second = fetch_ping_data(&test_client(), &server.base_url(), "2026-09-01")
            .expect("the second call must be served from the cache");
        assert_eq!(second.len(), 1);
        assert_eq!(second.signature(0), "OOM | small");

        let requests = server.requests();
        assert_eq!(
            requests.len(),
            1,
            "the second call went to the network instead of the cache"
        );
        assert_eq!(requests[0].path, "/ping_data/2026-09-01");
    }

    /// Pins the stack endpoint's shape, so a refactor cannot silently start
    /// querying the wrong URL while every status-mapping test above still
    /// passes.
    #[test]
    fn fetch_stack_requests_the_documented_path() {
        let server = TestServer::start();
        server.push_response(200, r#"{"stack": [], "java_exception": null}"#);

        let resp = fetch_stack(
            &test_client(),
            &server.base_url(),
            "2026-09-01",
            "ping-id-1",
        )
        .expect("an empty stack still parses");
        assert_eq!(resp.stack.map(|frames| frames.len()), Some(0));

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/stack/2026-09-01/ping-id-1");
    }
}
