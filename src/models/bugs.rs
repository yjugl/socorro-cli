// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct BugsResponse {
    pub hits: Vec<BugHit>,
    pub total: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BugHit {
    pub id: u64,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct BugsSummary {
    pub bugs: Vec<BugGroup>,
}

#[derive(Debug, Serialize)]
pub struct BugGroup {
    pub bug_id: u64,
    pub signatures: Vec<String>,
}

impl BugsResponse {
    pub fn to_summary(&self) -> BugsSummary {
        let mut by_bug: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        for hit in &self.hits {
            by_bug
                .entry(hit.id)
                .or_default()
                .push(hit.signature.clone());
        }

        let bugs = by_bug
            .into_iter()
            .map(|(bug_id, mut signatures)| {
                signatures.sort();
                BugGroup { bug_id, signatures }
            })
            .collect();

        BugsSummary { bugs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_bugs_response() {
        let json = r#"{
            "hits": [
                {"id": 999999, "signature": "OOM | small"},
                {"id": 999999, "signature": "OOM | large"},
                {"id": 888888, "signature": "OOM | small"}
            ],
            "total": 3
        }"#;
        let response: BugsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 3);
        assert_eq!(response.hits.len(), 3);
        assert_eq!(response.hits[0].id, 999999);
        assert_eq!(response.hits[0].signature, "OOM | small");
    }

    #[test]
    fn test_deserialize_empty_response() {
        let json = r#"{"hits": [], "total": 0}"#;
        let response: BugsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 0);
        assert!(response.hits.is_empty());
    }

    #[test]
    fn test_to_summary_groups_by_bug() {
        let response = BugsResponse {
            hits: vec![
                BugHit {
                    id: 999999,
                    signature: "OOM | small".to_string(),
                },
                BugHit {
                    id: 999999,
                    signature: "OOM | large".to_string(),
                },
                BugHit {
                    id: 888888,
                    signature: "OOM | small".to_string(),
                },
            ],
            total: 3,
        };
        let summary = response.to_summary();
        assert_eq!(summary.bugs.len(), 2);

        // BTreeMap orders by key, so 888888 comes first
        assert_eq!(summary.bugs[0].bug_id, 888888);
        assert_eq!(summary.bugs[0].signatures, vec!["OOM | small"]);

        assert_eq!(summary.bugs[1].bug_id, 999999);
        assert_eq!(
            summary.bugs[1].signatures,
            vec!["OOM | large", "OOM | small"]
        );
    }

    #[test]
    fn test_to_summary_empty() {
        let response = BugsResponse {
            hits: vec![],
            total: 0,
        };
        let summary = response.to_summary();
        assert!(summary.bugs.is_empty());
    }

    #[test]
    fn test_to_summary_signatures_sorted() {
        let response = BugsResponse {
            hits: vec![
                BugHit {
                    id: 100,
                    signature: "Zzz".to_string(),
                },
                BugHit {
                    id: 100,
                    signature: "Aaa".to_string(),
                },
                BugHit {
                    id: 100,
                    signature: "Mmm".to_string(),
                },
            ],
            total: 3,
        };
        let summary = response.to_summary();
        assert_eq!(summary.bugs[0].signatures, vec!["Aaa", "Mmm", "Zzz"]);
    }
}
