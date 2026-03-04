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
use crate::{Error, Result};

const BASE_URL: &str = "https://crash-pings.mozilla.org";

fn fetch_ping_data(client: &reqwest::blocking::Client, date: &str) -> Result<CrashPingsResponse> {
    let cache_key = format!("crash-pings-{}.json", date);

    // Try cache first
    if let Some(cached) = cache::read_cached(&cache_key) {
        let resp: CrashPingsResponse = serde_json::from_slice(&cached)
            .map_err(|e| Error::ParseError(format!("cached data parse error: {}", e)))?;
        return Ok(resp);
    }

    let url = format!("{}/ping_data/{}", BASE_URL, date);
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
                    String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
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
        _ => Err(Error::Http(response.error_for_status().unwrap_err())),
    }
}

fn fetch_stack(
    client: &reqwest::blocking::Client,
    date: &str,
    crash_id: &str,
) -> Result<CrashPingStackResponse> {
    let url = format!("{}/stack/{}/{}", BASE_URL, date, crash_id);
    let response = client.get(&url).send()?;

    match response.status() {
        StatusCode::OK => {
            let text = response.text()?;
            serde_json::from_str(&text)
                .map_err(|e| Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)])))
        }
        StatusCode::NOT_FOUND => Err(Error::NotFound(format!(
            "Stack not found for crash ping {} on {}",
            crash_id, date
        ))),
        _ => Err(Error::Http(response.error_for_status().unwrap_err())),
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
    items.sort_by(|a, b| b.1.cmp(&a.1));
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
        let resp = fetch_stack(&client, date_from, crash_id)?;
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
            match fetch_ping_data(&client, date) {
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
    use serde_json::json;

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
}
