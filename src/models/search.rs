use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::common::deserialize_string_or_number;

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub total: u64,
    pub hits: Vec<CrashHit>,
    #[serde(default)]
    pub facets: HashMap<String, Vec<FacetBucket>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashHit {
    pub uuid: String,
    pub date: String,
    pub signature: String,
    pub product: String,
    pub version: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub build_id: Option<String>,
    #[serde(default)]
    pub release_channel: Option<String>,
    #[serde(default)]
    pub platform_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FacetBucket {
    pub term: String,
    pub count: u64,
}

pub struct SearchParams {
    pub signature: Option<String>,
    pub product: String,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub cpu_arch: Option<String>,
    pub release_channel: Option<String>,
    pub platform_version: Option<String>,
    pub days: u32,
    pub limit: usize,
    pub facets: Vec<String>,
    pub facets_size: Option<usize>,
    pub sort: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_search_response() {
        let json = r#"{
            "total": 42,
            "hits": [
                {
                    "uuid": "247653e8-7a18-4836-97d1-42a720260120",
                    "date": "2024-01-15T10:30:00",
                    "signature": "mozilla::SomeFunction",
                    "product": "Firefox",
                    "version": "120.0",
                    "platform": "Windows"
                }
            ],
            "facets": {}
        }"#;

        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 42);
        assert_eq!(response.hits.len(), 1);
        assert_eq!(response.hits[0].uuid, "247653e8-7a18-4836-97d1-42a720260120");
        assert_eq!(response.hits[0].signature, "mozilla::SomeFunction");
    }

    #[test]
    fn test_deserialize_search_response_with_facets() {
        let json = r#"{
            "total": 100,
            "hits": [],
            "facets": {
                "version": [
                    {"term": "120.0", "count": 50},
                    {"term": "119.0", "count": 30},
                    {"term": "118.0", "count": 20}
                ],
                "platform": [
                    {"term": "Windows", "count": 60},
                    {"term": "Linux", "count": 40}
                ]
            }
        }"#;

        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 100);
        assert_eq!(response.facets.len(), 2);

        let version_facets = response.facets.get("version").unwrap();
        assert_eq!(version_facets.len(), 3);
        assert_eq!(version_facets[0].term, "120.0");
        assert_eq!(version_facets[0].count, 50);
    }

    #[test]
    fn test_deserialize_crash_hit_missing_platform() {
        let json = r#"{
            "uuid": "test-id",
            "date": "2024-01-15",
            "signature": "crash_sig",
            "product": "Firefox",
            "version": "120.0"
        }"#;

        let hit: CrashHit = serde_json::from_str(json).unwrap();
        assert_eq!(hit.platform, None);
    }

    #[test]
    fn test_deserialize_empty_response() {
        let json = r#"{
            "total": 0,
            "hits": [],
            "facets": {}
        }"#;

        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 0);
        assert!(response.hits.is_empty());
        assert!(response.facets.is_empty());
    }
}
