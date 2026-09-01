// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::Result;
use crate::models::bugs::BugsResponse;
use crate::models::crash_pings::{CrashPingStackSummary, CrashPingsSummary};
use crate::models::{CorrelationsResponse, SearchResponse};

pub fn format_bugs(response: &BugsResponse) -> Result<String> {
    Ok(serde_json::to_string_pretty(response)?)
}

/// Pretty-print a raw `/ProcessedCrash/` response verbatim.
///
/// Emits every key the server sent, including the ones the `ProcessedCrash`
/// struct does not declare and would therefore drop if it were serialized
/// instead. Output is pretty-printed for consistency with every other formatter
/// in this module.
pub fn format_crash_raw(value: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

pub fn format_search(response: &SearchResponse) -> Result<String> {
    Ok(serde_json::to_string_pretty(response)?)
}

pub fn format_correlations(response: &CorrelationsResponse) -> Result<String> {
    Ok(serde_json::to_string_pretty(response)?)
}

pub fn format_crash_pings(summary: &CrashPingsSummary) -> Result<String> {
    Ok(serde_json::to_string_pretty(summary)?)
}

pub fn format_crash_ping_stack(summary: &CrashPingStackSummary) -> Result<String> {
    Ok(serde_json::to_string_pretty(summary)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed-down stand-in for a raw `/ProcessedCrash/` response: it mixes
    /// keys the `ProcessedCrash` struct declares (`uuid`, `signature`) with keys
    /// it does not (`async_shutdown_timeout`, `app_notes`, `proto_signature`,
    /// `thread_count`, `uptime`, `telemetry_environment`), which are exactly the
    /// ones a formatter serializing that struct would drop.
    const RAW_FIXTURE: &str = r#"{
      "uuid": "b98bbb81-3ff6-4825-991f-6a0b30260901",
      "async_shutdown_timeout": "{\"phase\": \"profile-before-change\"}",
      "app_notes": "note",
      "proto_signature": "a | b",
      "thread_count": 64,
      "uptime": 1234,
      "telemetry_environment": {"build": {"version": "145.0"}},
      "signature": "nsThread::ProcessNextEvent"
    }"#;

    #[test]
    fn test_format_crash_raw_preserves_every_key() {
        let value: serde_json::Value = serde_json::from_str(RAW_FIXTURE).unwrap();
        let output = format_crash_raw(&value).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Nothing dropped, nothing invented, no value altered.
        assert_eq!(reparsed, value);

        let mut in_keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut out_keys: Vec<&str> = reparsed
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        in_keys.sort_unstable();
        out_keys.sort_unstable();
        assert_eq!(out_keys, in_keys);
        assert_eq!(out_keys.len(), 8);

        // The keys serializing the struct would have dropped are all present.
        for key in [
            "async_shutdown_timeout",
            "app_notes",
            "proto_signature",
            "thread_count",
            "uptime",
            "telemetry_environment",
        ] {
            assert!(
                output.contains(&format!("\"{}\"", key)),
                "raw passthrough dropped {}",
                key
            );
        }
        assert!(output.contains("\"thread_count\": 64"));
        assert!(output.contains("\"proto_signature\": \"a | b\""));
        // Nested objects survive, not just scalars.
        assert!(output.contains("\"version\": \"145.0\""));
    }

    #[test]
    fn test_format_crash_raw_is_pretty_printed() {
        let value: serde_json::Value = serde_json::from_str(RAW_FIXTURE).unwrap();
        let output = format_crash_raw(&value).unwrap();

        // Pretty-printed with two-space indentation, like every other formatter
        // in this module, rather than compact single-line JSON.
        assert!(output.starts_with("{\n"));
        assert!(output.contains("\n  \"uuid\": "));
        assert!(output.lines().count() > 8);
    }
}
