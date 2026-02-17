use crate::models::{CorrelationsResponse, ProcessedCrash, SearchResponse};
use crate::models::crash_pings::{CrashPingFilters, CrashPingStackSummary, CrashPingsResponse};
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

pub fn format_crash_pings(
    response: &CrashPingsResponse,
    filters: &CrashPingFilters,
    facet: &str,
    limit: usize,
    date: &str,
) -> Result<String> {
    // Build a JSON object with the aggregated data
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut filtered_total = 0usize;

    for i in 0..response.len() {
        if !response.matches_filters(i, filters) {
            continue;
        }
        filtered_total += 1;
        let value = response.facet_value(i, facet);
        *counts.entry(value).or_insert(0) += 1;
    }

    let mut items: Vec<(String, usize)> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1));
    items.truncate(limit);

    let json_items: Vec<serde_json::Value> = items
        .into_iter()
        .map(|(label, count)| {
            let pct = if filtered_total > 0 {
                count as f64 / filtered_total as f64 * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "label": label,
                "count": count,
                "percentage": (pct * 100.0).round() / 100.0,
            })
        })
        .collect();

    let result = serde_json::json!({
        "date": date,
        "total": response.len(),
        "filtered_total": filtered_total,
        "facet": facet,
        "items": json_items,
    });

    Ok(serde_json::to_string_pretty(&result)?)
}

pub fn format_crash_ping_stack(summary: &CrashPingStackSummary) -> Result<String> {
    Ok(serde_json::to_string_pretty(summary)?)
}
