use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct CorrelationsTotals {
    pub date: String,
    pub release: u64,
    pub beta: u64,
    pub nightly: u64,
    pub esr: u64,
}

impl CorrelationsTotals {
    pub fn total_for_channel(&self, channel: &str) -> Option<u64> {
        match channel {
            "release" => Some(self.release),
            "beta" => Some(self.beta),
            "nightly" => Some(self.nightly),
            "esr" => Some(self.esr),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CorrelationsResponse {
    pub total: f64,
    pub results: Vec<CorrelationResult>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CorrelationResult {
    pub item: HashMap<String, serde_json::Value>,
    pub count_reference: f64,
    pub count_group: f64,
    pub prior: Option<CorrelationPrior>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CorrelationPrior {
    pub item: HashMap<String, serde_json::Value>,
    pub count_reference: f64,
    pub count_group: f64,
    pub total_reference: f64,
    pub total_group: f64,
}

#[derive(Debug)]
pub struct CorrelationsSummary {
    pub signature: String,
    pub channel: String,
    pub date: String,
    pub sig_count: f64,
    pub ref_count: u64,
    pub items: Vec<CorrelationItem>,
}

#[derive(Debug)]
pub struct CorrelationItem {
    pub label: String,
    pub sig_pct: f64,
    pub ref_pct: f64,
    pub prior: Option<CorrelationItemPrior>,
}

#[derive(Debug)]
pub struct CorrelationItemPrior {
    pub label: String,
    pub sig_pct: f64,
    pub ref_pct: f64,
}

pub fn format_item_map(item: &HashMap<String, serde_json::Value>) -> String {
    let mut keys: Vec<&String> = item.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .iter()
        .map(|k| {
            let v = &item[*k];
            let val_str = match v {
                serde_json::Value::Null => "null".to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            format!("{} = {}", k, val_str)
        })
        .collect();
    parts.join(" \u{2227} ")
}

impl CorrelationsResponse {
    pub fn to_summary(
        &self,
        signature: &str,
        channel: &str,
        totals: &CorrelationsTotals,
    ) -> CorrelationsSummary {
        let ref_count = totals.total_for_channel(channel).unwrap_or(0);
        let items = self
            .results
            .iter()
            .map(|r| {
                let sig_pct = if self.total > 0.0 {
                    r.count_group / self.total * 100.0
                } else {
                    0.0
                };
                let ref_pct = if ref_count > 0 {
                    r.count_reference / ref_count as f64 * 100.0
                } else {
                    0.0
                };
                let prior = r.prior.as_ref().map(|p| {
                    let prior_sig_pct = if p.total_group > 0.0 {
                        p.count_group / p.total_group * 100.0
                    } else {
                        0.0
                    };
                    let prior_ref_pct = if p.total_reference > 0.0 {
                        p.count_reference / p.total_reference * 100.0
                    } else {
                        0.0
                    };
                    CorrelationItemPrior {
                        label: format_item_map(&p.item),
                        sig_pct: prior_sig_pct,
                        ref_pct: prior_ref_pct,
                    }
                });
                CorrelationItem {
                    label: format_item_map(&r.item),
                    sig_pct,
                    ref_pct,
                    prior,
                }
            })
            .collect();

        CorrelationsSummary {
            signature: signature.to_string(),
            channel: channel.to_string(),
            date: totals.date.clone(),
            sig_count: self.total,
            ref_count,
            items,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deserialize_totals() {
        let data = r#"{"date":"2026-02-13","release":79268,"beta":4996,"nightly":4876,"esr":792}"#;
        let totals: CorrelationsTotals = serde_json::from_str(data).unwrap();
        assert_eq!(totals.date, "2026-02-13");
        assert_eq!(totals.release, 79268);
        assert_eq!(totals.beta, 4996);
        assert_eq!(totals.nightly, 4876);
        assert_eq!(totals.esr, 792);
    }

    #[test]
    fn test_total_for_channel_valid() {
        let totals = CorrelationsTotals {
            date: "2026-02-13".to_string(),
            release: 79268,
            beta: 4996,
            nightly: 4876,
            esr: 792,
        };
        assert_eq!(totals.total_for_channel("release"), Some(79268));
        assert_eq!(totals.total_for_channel("beta"), Some(4996));
        assert_eq!(totals.total_for_channel("nightly"), Some(4876));
        assert_eq!(totals.total_for_channel("esr"), Some(792));
    }

    #[test]
    fn test_total_for_channel_invalid() {
        let totals = CorrelationsTotals {
            date: "2026-02-13".to_string(),
            release: 79268,
            beta: 4996,
            nightly: 4876,
            esr: 792,
        };
        assert_eq!(totals.total_for_channel("aurora"), None);
        assert_eq!(totals.total_for_channel("unknown"), None);
    }

    #[test]
    fn test_deserialize_correlations_response() {
        let data = r#"{
            "total": 220.0,
            "results": [
                {
                    "item": {"Module \"cscapi.dll\"": true},
                    "count_reference": 19432.0,
                    "count_group": 220.0,
                    "prior": null
                },
                {
                    "item": {"startup_crash": null},
                    "count_reference": 920.0,
                    "count_group": 65.0,
                    "prior": {
                        "item": {"process_type": "parent"},
                        "count_reference": 3630.0,
                        "count_group": 112.0,
                        "total_reference": 79268.0,
                        "total_group": 220.0
                    }
                }
            ]
        }"#;
        let resp: CorrelationsResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.total, 220.0);
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].count_group, 220.0);
        assert!(resp.results[0].prior.is_none());
        assert!(resp.results[1].prior.is_some());
    }

    #[test]
    fn test_to_summary_percentages() {
        let totals = CorrelationsTotals {
            date: "2026-02-13".to_string(),
            release: 79268,
            beta: 4996,
            nightly: 4876,
            esr: 792,
        };
        let mut item = HashMap::new();
        item.insert("Module \"cscapi.dll\"".to_string(), json!(true));
        let resp = CorrelationsResponse {
            total: 220.0,
            results: vec![CorrelationResult {
                item,
                count_reference: 19432.0,
                count_group: 220.0,
                prior: None,
            }],
        };
        let summary = resp.to_summary("TestSig", "release", &totals);
        assert_eq!(summary.sig_count, 220.0);
        assert_eq!(summary.ref_count, 79268);
        assert!((summary.items[0].sig_pct - 100.0).abs() < 0.01);
        assert!((summary.items[0].ref_pct - 24.51).abs() < 0.01);
    }

    #[test]
    fn test_to_summary_with_prior() {
        let totals = CorrelationsTotals {
            date: "2026-02-13".to_string(),
            release: 79268,
            beta: 4996,
            nightly: 4876,
            esr: 792,
        };
        let mut item = HashMap::new();
        item.insert("startup_crash".to_string(), serde_json::Value::Null);
        let mut prior_item = HashMap::new();
        prior_item.insert("process_type".to_string(), json!("parent"));
        let resp = CorrelationsResponse {
            total: 220.0,
            results: vec![CorrelationResult {
                item,
                count_reference: 920.0,
                count_group: 65.0,
                prior: Some(CorrelationPrior {
                    item: prior_item,
                    count_reference: 3630.0,
                    count_group: 112.0,
                    total_reference: 79268.0,
                    total_group: 220.0,
                }),
            }],
        };
        let summary = resp.to_summary("TestSig", "release", &totals);
        let item = &summary.items[0];
        assert!((item.sig_pct - 29.545).abs() < 0.01);
        let prior = item.prior.as_ref().unwrap();
        assert!((prior.sig_pct - 50.909).abs() < 0.01);
        assert!((prior.ref_pct - 4.578).abs() < 0.01);
    }

    #[test]
    fn test_format_item_map_single_key_true() {
        let mut item = HashMap::new();
        item.insert("Module \"cscapi.dll\"".to_string(), json!(true));
        assert_eq!(format_item_map(&item), "Module \"cscapi.dll\" = true");
    }

    #[test]
    fn test_format_item_map_single_key_null() {
        let mut item = HashMap::new();
        item.insert("startup_crash".to_string(), serde_json::Value::Null);
        assert_eq!(format_item_map(&item), "startup_crash = null");
    }

    #[test]
    fn test_format_item_map_single_key_string() {
        let mut item = HashMap::new();
        item.insert("process_type".to_string(), json!("parent"));
        assert_eq!(format_item_map(&item), "process_type = parent");
    }

    #[test]
    fn test_format_item_map_multi_key_sorted() {
        let mut item = HashMap::new();
        item.insert("z_field".to_string(), json!(true));
        item.insert("a_field".to_string(), json!("value"));
        let result = format_item_map(&item);
        assert_eq!(result, "a_field = value \u{2227} z_field = true");
    }
}
