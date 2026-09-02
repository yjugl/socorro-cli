// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::fmt::Write;

use reqwest::StatusCode;
use sha1::{Digest, Sha1};

use crate::models::{CorrelationsResponse, CorrelationsTotals};
use crate::output::{OutputFormat, compact, json, markdown};
use crate::{Error, PREVIEW_BYTES, Result, status_error, truncate_on_char_boundary};

const CDN_BASE: &str =
    "https://analysis-output.telemetry.mozilla.org/top-signatures-correlations/data";

pub fn signature_hash(sig: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(sig.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        write!(out, "{:02x}", byte).unwrap();
    }
    out
}

/// Fetch the per-channel crash totals from the CDN's `all.json.gz`.
///
/// Only `200` is a success here; every other status, including a `404` for a
/// missing object, is classified by [`status_error`].
fn fetch_totals(client: &reqwest::blocking::Client, base_url: &str) -> Result<CorrelationsTotals> {
    let url = format!("{}/all.json.gz", base_url);
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
        _ => Err(status_error(response)),
    }
}

/// Fetch one signature's correlations from `<channel>/<sha1>.json.gz`.
///
/// A `404` is expected rather than exceptional -- the CDN only publishes the
/// top ~200 signatures per channel -- so it maps to [`Error::NotFound`] with an
/// explanation. Everything else is classified by [`status_error`].
fn fetch_signature_correlations(
    client: &reqwest::blocking::Client,
    base_url: &str,
    signature: &str,
    channel: &str,
) -> Result<CorrelationsResponse> {
    let hash = signature_hash(signature);
    let url = format!("{}/{}/{}.json.gz", base_url, channel, hash);
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
            "No correlation data for signature \"{}\" on channel \"{}\". \
             Correlations are only available for the top ~200 signatures per channel.",
            signature, channel
        ))),
        _ => Err(status_error(response)),
    }
}

pub fn execute(signature: &str, channel: &str, format: OutputFormat) -> Result<()> {
    let client = reqwest::blocking::Client::builder().gzip(true).build()?;

    let totals = fetch_totals(&client, CDN_BASE)?;

    if totals.total_for_channel(channel).is_none() {
        return Err(Error::ParseError(format!(
            "Unknown channel \"{}\". Valid channels: release, beta, nightly, esr",
            channel
        )));
    }

    let response = fetch_signature_correlations(&client, CDN_BASE, signature, channel)?;

    let output = match format {
        OutputFormat::Compact => {
            let summary = response.to_summary(signature, channel, &totals);
            compact::format_correlations(&summary)
        }
        OutputFormat::Json => json::format_correlations(&response)?,
        OutputFormat::Markdown => {
            let summary = response.to_summary(signature, channel, &totals);
            markdown::format_correlations(&summary)
        }
    };

    print!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use crate::test_server::TestServer;

    /// A valid `all.json.gz` payload, in the shape the CDN serves.
    const TOTALS_BODY: &str =
        r#"{"date":"2026-09-01","release":14,"beta":1234,"nightly":4503,"esr":42}"#;

    /// A valid per-signature payload with one result and no prior.
    const SIGNATURE_BODY: &str = r#"{"total":100.0,"results":[{"item":{"platform_pretty_version":"Windows 11"},"count_reference":12.5,"count_group":87.5,"prior":null}]}"#;

    /// Mirrors the client `execute` builds (`.gzip(true)`), but with proxies
    /// disabled: the test server is on loopback, and an `http_proxy` in the
    /// environment would otherwise send the request somewhere else entirely.
    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .gzip(true)
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build blocking test client")
    }

    /// A body that is not JSON and whose byte 200 lands inside a multi-byte
    /// character: 199 ASCII bytes then a three-byte em dash spanning 199..202.
    /// This is the exact shape that makes a naive `&text[..200]` preview slice
    /// panic with "end byte index 200 is not a char boundary".
    fn body_split_mid_utf8_sequence() -> String {
        let body = format!("{}\u{2014}", "a".repeat(199));
        assert_eq!(body.len(), 202);
        assert!(!body.is_char_boundary(200));
        body
    }

    #[test]
    fn fetch_totals_reports_a_body_split_mid_utf8_as_a_parse_error() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_utf8_sequence());

        let error = fetch_totals(&client(), &server.base_url())
            .expect_err("a non-JSON body must not parse as totals");

        match error {
            Error::ParseError(message) => {
                // The preview stopped at the character boundary below the cap.
                assert!(
                    message.contains(&"a".repeat(199)),
                    "preview did not include the ASCII prefix: {message}"
                );
                assert!(
                    !message.contains('\u{2014}'),
                    "preview should have stopped before the em dash: {message}"
                );
            }
            other => panic!("expected Error::ParseError, got {other:?}"),
        }
    }

    #[test]
    fn fetch_signature_correlations_reports_a_body_split_mid_utf8_as_a_parse_error() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_utf8_sequence());

        let error =
            fetch_signature_correlations(&client(), &server.base_url(), "OOM | small", "nightly")
                .expect_err("a non-JSON body must not parse as correlations");

        match error {
            Error::ParseError(message) => {
                assert!(
                    message.contains(&"a".repeat(199)),
                    "preview did not include the ASCII prefix: {message}"
                );
                assert!(
                    !message.contains('\u{2014}'),
                    "preview should have stopped before the em dash: {message}"
                );
            }
            other => panic!("expected Error::ParseError, got {other:?}"),
        }
    }

    #[test]
    fn fetch_totals_parses_a_valid_payload_from_all_json_gz() {
        let server = TestServer::start();
        server.push_response(200, TOTALS_BODY);

        let totals = fetch_totals(&client(), &server.base_url()).expect("valid totals payload");

        assert_eq!(totals.date, "2026-09-01");
        assert_eq!(totals.total_for_channel("release"), Some(14));
        assert_eq!(totals.total_for_channel("nightly"), Some(4503));

        // Pin the CDN object the totals fetch asks for, so a refactor cannot
        // silently start querying a different one.
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/all.json.gz");
    }

    #[test]
    fn fetch_signature_correlations_parses_a_valid_payload_from_the_hashed_object() {
        let server = TestServer::start();
        server.push_response(200, SIGNATURE_BODY);

        let response = fetch_signature_correlations(
            &client(),
            &server.base_url(),
            "UiaNode::ProviderInfo::~ProviderInfo",
            "nightly",
        )
        .expect("valid correlations payload");

        assert_eq!(response.total, 100.0);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].count_group, 87.5);

        // The per-signature object is `<channel>/<sha1 of the signature>.json.gz`.
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/nightly/4361bb82d8d8c7f34466f8b7589fbd6c920da702.json.gz"
        );
    }

    #[test]
    fn fetch_totals_maps_an_unhandled_success_status_to_unexpected_status() {
        let server = TestServer::start();
        // 202 is neither a client nor a server error, so `error_for_status`
        // hands back `Ok`; the old `_` arm unwrapped that as an error.
        server.push_response(202, "accepted, not ready yet");

        let error = fetch_totals(&client(), &server.base_url())
            .expect_err("a 202 must not be reported as totals");

        match error {
            Error::UnexpectedStatus { status, url } => {
                assert_eq!(status, 202);
                assert_eq!(url, format!("{}/all.json.gz", server.base_url()));
            }
            other => panic!("expected Error::UnexpectedStatus, got {other:?}"),
        }
    }

    #[test]
    fn fetch_signature_correlations_maps_an_unhandled_success_status_to_unexpected_status() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error =
            fetch_signature_correlations(&client(), &server.base_url(), "OOM | small", "nightly")
                .expect_err("a 202 must not be reported as correlations");

        match error {
            Error::UnexpectedStatus { status, url } => {
                assert_eq!(status, 202);
                assert!(
                    url.ends_with(".json.gz"),
                    "unexpected-status url should name the CDN object: {url}"
                );
            }
            other => panic!("expected Error::UnexpectedStatus, got {other:?}"),
        }
    }

    #[test]
    fn fetch_totals_maps_a_server_error_to_http() {
        let server = TestServer::start();
        server.push_response(500, "upstream exploded");

        let error = fetch_totals(&client(), &server.base_url())
            .expect_err("a 500 must not be reported as totals");

        // A genuine 5xx keeps reqwest's richer error rather than being
        // flattened into UnexpectedStatus.
        match error {
            Error::Http(err) => {
                assert_eq!(err.status().map(|s| s.as_u16()), Some(500));
            }
            other => panic!("expected Error::Http, got {other:?}"),
        }
    }

    #[test]
    fn fetch_signature_correlations_maps_a_server_error_to_http() {
        let server = TestServer::start();
        server.push_response(503, "try later");

        let error =
            fetch_signature_correlations(&client(), &server.base_url(), "OOM | small", "nightly")
                .expect_err("a 503 must not be reported as correlations");

        match error {
            Error::Http(err) => {
                assert_eq!(err.status().map(|s| s.as_u16()), Some(503));
            }
            other => panic!("expected Error::Http, got {other:?}"),
        }
    }

    #[test]
    fn fetch_totals_maps_404_to_http_through_its_fallthrough() {
        let server = TestServer::start();
        server.push_response(404, "no such object");

        let error = fetch_totals(&client(), &server.base_url())
            .expect_err("a 404 must not be reported as totals");

        // `fetch_totals` handles only 200 explicitly, so a missing
        // `all.json.gz` has always been an Error::Http. Keep it there: the
        // unexpected-status path is for statuses reqwest does not treat as
        // errors at all.
        match error {
            Error::Http(err) => {
                assert_eq!(err.status().map(|s| s.as_u16()), Some(404));
            }
            other => panic!("expected Error::Http, got {other:?}"),
        }
    }

    #[test]
    fn fetch_signature_correlations_maps_404_to_not_found() {
        let server = TestServer::start();
        server.push_response(404, "no such object");

        let error =
            fetch_signature_correlations(&client(), &server.base_url(), "OOM | small", "nightly")
                .expect_err("a 404 must not be reported as correlations");

        match error {
            Error::NotFound(message) => {
                assert!(message.contains("OOM | small"), "{message}");
                assert!(message.contains("nightly"), "{message}");
                assert!(message.contains("top ~200 signatures"), "{message}");
            }
            other => panic!("expected Error::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_signature_hash() {
        assert_eq!(
            signature_hash("UiaNode::ProviderInfo::~ProviderInfo"),
            "4361bb82d8d8c7f34466f8b7589fbd6c920da702"
        );
    }

    #[test]
    fn test_signature_hash_oom() {
        let hash = signature_hash("OOM | small");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 40);
    }
}
