use crate::models::crash_pings::{CrashPingStackSummary, CrashPingsSummary};
use crate::models::{CorrelationsResponse, ProcessedCrash, SearchResponse};
use crate::Result;

pub fn format_crash(crash: &ProcessedCrash) -> Result<String> {
    Ok(serde_json::to_string_pretty(crash)?)
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
