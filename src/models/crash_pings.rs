use serde::{Deserialize, Serialize};

// --- API response types (struct-of-arrays with string deduplication) ---

#[derive(Debug, Deserialize, Serialize)]
pub struct IndexedStrings {
    pub strings: Vec<String>,
    pub values: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NullableIndexedStrings {
    pub strings: Vec<Option<String>>,
    pub values: Vec<u32>,
}

impl IndexedStrings {
    pub fn get(&self, i: usize) -> &str {
        &self.strings[self.values[i] as usize]
    }
}

impl NullableIndexedStrings {
    pub fn get(&self, i: usize) -> Option<&str> {
        self.strings[self.values[i] as usize].as_deref()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashPingsResponse {
    pub channel: IndexedStrings,
    pub process: IndexedStrings,
    pub ipc_actor: NullableIndexedStrings,
    pub clientid: IndexedStrings,
    pub crashid: Vec<String>,
    pub version: IndexedStrings,
    pub os: IndexedStrings,
    pub osversion: IndexedStrings,
    pub arch: IndexedStrings,
    pub date: IndexedStrings,
    pub reason: NullableIndexedStrings,
    #[serde(rename = "type")]
    pub crash_type: NullableIndexedStrings,
    pub minidump_sha256_hash: Vec<Option<String>>,
    pub startup_crash: Vec<Option<bool>>,
    pub build_id: IndexedStrings,
    pub signature: IndexedStrings,
}

impl CrashPingsResponse {
    pub fn len(&self) -> usize {
        self.crashid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.crashid.is_empty()
    }

    pub fn signature(&self, i: usize) -> &str {
        self.signature.get(i)
    }

    pub fn channel(&self, i: usize) -> &str {
        self.channel.get(i)
    }

    pub fn os(&self, i: usize) -> &str {
        self.os.get(i)
    }

    pub fn process(&self, i: usize) -> &str {
        self.process.get(i)
    }

    pub fn version(&self, i: usize) -> &str {
        self.version.get(i)
    }

    pub fn arch(&self, i: usize) -> &str {
        self.arch.get(i)
    }

    pub fn matches_filters(&self, i: usize, filters: &CrashPingFilters) -> bool {
        if let Some(ref ch) = filters.channel {
            if !self.channel(i).eq_ignore_ascii_case(ch) {
                return false;
            }
        }
        if let Some(ref os) = filters.os {
            if !self.os(i).eq_ignore_ascii_case(os) {
                return false;
            }
        }
        if let Some(ref proc) = filters.process {
            if !self.process(i).eq_ignore_ascii_case(proc) {
                return false;
            }
        }
        if let Some(ref ver) = filters.version {
            if self.version(i) != ver {
                return false;
            }
        }
        if let Some(ref sig) = filters.signature {
            let ping_sig = self.signature(i);
            if let Some(pattern) = sig.strip_prefix('~') {
                if !ping_sig.to_lowercase().contains(&pattern.to_lowercase()) {
                    return false;
                }
            } else if ping_sig != sig {
                return false;
            }
        }
        if let Some(ref arch) = filters.arch {
            if !self.arch(i).eq_ignore_ascii_case(arch) {
                return false;
            }
        }
        true
    }

    pub fn facet_value(&self, i: usize, facet: &str) -> String {
        match facet {
            "signature" => self.signature(i).to_string(),
            "channel" => self.channel(i).to_string(),
            "os" => self.os(i).to_string(),
            "process" => self.process(i).to_string(),
            "version" => self.version(i).to_string(),
            "arch" => self.arch(i).to_string(),
            "osversion" => self.osversion.get(i).to_string(),
            "build_id" => self.build_id.get(i).to_string(),
            "ipc_actor" => self.ipc_actor.get(i).unwrap_or("(none)").to_string(),
            "reason" => self.reason.get(i).unwrap_or("(none)").to_string(),
            "type" => self.crash_type.get(i).unwrap_or("(none)").to_string(),
            _ => "(unknown facet)".to_string(),
        }
    }
}

// --- Stack trace types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashPingStackResponse {
    pub stack: Option<Vec<CrashPingFrame>>,
    pub java_exception: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashPingFrame {
    pub function: Option<String>,
    pub function_offset: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub module: Option<String>,
    pub module_offset: Option<String>,
    pub offset: Option<String>,
    #[serde(default)]
    pub omitted: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

// --- Filter parameters ---

#[derive(Debug, Default)]
pub struct CrashPingFilters {
    pub channel: Option<String>,
    pub os: Option<String>,
    pub process: Option<String>,
    pub version: Option<String>,
    pub signature: Option<String>,
    pub arch: Option<String>,
}

// --- Summary types for display ---

#[derive(Debug)]
pub struct CrashPingsSummary {
    pub date: String,
    pub total: usize,
    pub filtered_total: usize,
    pub signature_filter: Option<String>,
    pub facet_name: String,
    pub items: Vec<CrashPingsItem>,
}

#[derive(Debug)]
pub struct CrashPingsItem {
    pub label: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct CrashPingStackSummary {
    pub crash_id: String,
    pub date: String,
    pub frames: Vec<CrashPingFrame>,
    pub java_exception: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_response_json() -> serde_json::Value {
        json!({
            "channel": {
                "strings": ["release", "beta", "nightly"],
                "values": [0, 0, 1, 2]
            },
            "process": {
                "strings": ["main", "content", "gpu"],
                "values": [0, 1, 0, 2]
            },
            "ipc_actor": {
                "strings": [null, "windows-file-dialog"],
                "values": [0, 1, 0, 0]
            },
            "clientid": {
                "strings": ["client1", "client2", "client3", "client4"],
                "values": [0, 1, 2, 3]
            },
            "crashid": ["crash-1", "crash-2", "crash-3", "crash-4"],
            "version": {
                "strings": ["147.0", "148.0"],
                "values": [0, 0, 1, 1]
            },
            "os": {
                "strings": ["Windows", "Linux", "Mac"],
                "values": [0, 0, 1, 2]
            },
            "osversion": {
                "strings": ["10.0.19045", "6.1", "15.0"],
                "values": [0, 0, 1, 2]
            },
            "arch": {
                "strings": ["x86_64", "aarch64"],
                "values": [0, 0, 0, 1]
            },
            "date": {
                "strings": ["2026-02-12"],
                "values": [0, 0, 0, 0]
            },
            "reason": {
                "strings": [null, "OOM"],
                "values": [0, 1, 0, 1]
            },
            "type": {
                "strings": [null, "SIGSEGV"],
                "values": [0, 1, 0, 0]
            },
            "minidump_sha256_hash": ["hash1", null, "hash3", null],
            "startup_crash": [false, true, false, false],
            "build_id": {
                "strings": ["20260210103000", "20260211103000"],
                "values": [0, 0, 1, 1]
            },
            "signature": {
                "strings": ["OOM | small", "setup_stack_prot", "js::gc::SomeFunc"],
                "values": [0, 0, 1, 2]
            }
        })
    }

    #[test]
    fn test_deserialize_response() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.len(), 4);
        assert_eq!(resp.crashid.len(), 4);
    }

    #[test]
    fn test_indexed_strings_get() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.signature(0), "OOM | small");
        assert_eq!(resp.signature(1), "OOM | small");
        assert_eq!(resp.signature(2), "setup_stack_prot");
        assert_eq!(resp.signature(3), "js::gc::SomeFunc");
    }

    #[test]
    fn test_nullable_indexed_strings() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.ipc_actor.get(0), None);
        assert_eq!(resp.ipc_actor.get(1), Some("windows-file-dialog"));
    }

    #[test]
    fn test_channel_accessor() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.channel(0), "release");
        assert_eq!(resp.channel(2), "beta");
        assert_eq!(resp.channel(3), "nightly");
    }

    #[test]
    fn test_os_accessor() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.os(0), "Windows");
        assert_eq!(resp.os(2), "Linux");
        assert_eq!(resp.os(3), "Mac");
    }

    #[test]
    fn test_filter_no_filters() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters::default();
        for i in 0..resp.len() {
            assert!(resp.matches_filters(i, &filters));
        }
    }

    #[test]
    fn test_filter_by_channel() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            channel: Some("release".to_string()),
            ..Default::default()
        };
        assert!(resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(1, &filters));
        assert!(!resp.matches_filters(2, &filters));
        assert!(!resp.matches_filters(3, &filters));
    }

    #[test]
    fn test_filter_by_os() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            os: Some("Linux".to_string()),
            ..Default::default()
        };
        assert!(!resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(2, &filters));
    }

    #[test]
    fn test_filter_by_signature_exact() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            signature: Some("OOM | small".to_string()),
            ..Default::default()
        };
        assert!(resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(1, &filters));
        assert!(!resp.matches_filters(2, &filters));
    }

    #[test]
    fn test_filter_by_signature_contains() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            signature: Some("~oom".to_string()),
            ..Default::default()
        };
        assert!(resp.matches_filters(0, &filters));
        assert!(!resp.matches_filters(2, &filters));
    }

    #[test]
    fn test_filter_combined() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            channel: Some("release".to_string()),
            os: Some("Windows".to_string()),
            ..Default::default()
        };
        assert!(resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(1, &filters));
        assert!(!resp.matches_filters(2, &filters)); // beta
        assert!(!resp.matches_filters(3, &filters)); // nightly + Mac
    }

    #[test]
    fn test_filter_case_insensitive() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            os: Some("windows".to_string()),
            ..Default::default()
        };
        assert!(resp.matches_filters(0, &filters));
    }

    #[test]
    fn test_facet_value() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.facet_value(0, "signature"), "OOM | small");
        assert_eq!(resp.facet_value(0, "os"), "Windows");
        assert_eq!(resp.facet_value(0, "channel"), "release");
        assert_eq!(resp.facet_value(1, "ipc_actor"), "windows-file-dialog");
        assert_eq!(resp.facet_value(0, "ipc_actor"), "(none)");
    }

    #[test]
    fn test_deserialize_stack_response() {
        let data = json!({
            "stack": [
                {
                    "function": "KiRaiseUserExceptionDispatcher",
                    "function_offset": "0x000000000000003a",
                    "file": null,
                    "line": null,
                    "module": "ntdll.dll",
                    "module_offset": "0x00000000000a14fa",
                    "omitted": null,
                    "error": null,
                    "offset": "0x00007ffbeeef14fa"
                },
                {
                    "function": "mozilla::SomeFunc",
                    "function_offset": null,
                    "file": "SomeFile.cpp",
                    "line": 42,
                    "module": "xul.dll",
                    "module_offset": "0x1234",
                    "omitted": null,
                    "error": null,
                    "offset": "0x1234"
                }
            ],
            "java_exception": null
        });
        let resp: CrashPingStackResponse = serde_json::from_value(data).unwrap();
        let stack = resp.stack.unwrap();
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[0].function.as_deref(), Some("KiRaiseUserExceptionDispatcher"));
        assert_eq!(stack[0].module.as_deref(), Some("ntdll.dll"));
        assert!(stack[0].file.is_none());
        assert_eq!(stack[1].file.as_deref(), Some("SomeFile.cpp"));
        assert_eq!(stack[1].line, Some(42));
    }

    #[test]
    fn test_deserialize_stack_response_null_stack() {
        let data = json!({
            "stack": null,
            "java_exception": null
        });
        let resp: CrashPingStackResponse = serde_json::from_value(data).unwrap();
        assert!(resp.stack.is_none());
    }

    #[test]
    fn test_deserialize_stack_response_with_java_exception() {
        let data = json!({
            "stack": null,
            "java_exception": {"message": "OutOfMemoryError", "frames": []}
        });
        let resp: CrashPingStackResponse = serde_json::from_value(data).unwrap();
        assert!(resp.java_exception.is_some());
    }

    #[test]
    fn test_crash_pings_summary() {
        let summary = CrashPingsSummary {
            date: "2026-02-12".to_string(),
            total: 88808,
            filtered_total: 4523,
            signature_filter: Some("OOM | small".to_string()),
            facet_name: "os".to_string(),
            items: vec![
                CrashPingsItem {
                    label: "Windows".to_string(),
                    count: 3900,
                    percentage: 86.24,
                },
                CrashPingsItem {
                    label: "Linux".to_string(),
                    count: 400,
                    percentage: 8.85,
                },
            ],
        };
        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[0].label, "Windows");
    }

    #[test]
    fn test_filter_by_version() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            version: Some("148.0".to_string()),
            ..Default::default()
        };
        assert!(!resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(2, &filters));
        assert!(resp.matches_filters(3, &filters));
    }

    #[test]
    fn test_filter_by_arch() {
        let data = sample_response_json();
        let resp: CrashPingsResponse = serde_json::from_value(data).unwrap();
        let filters = CrashPingFilters {
            arch: Some("aarch64".to_string()),
            ..Default::default()
        };
        assert!(!resp.matches_filters(0, &filters));
        assert!(resp.matches_filters(3, &filters));
    }
}
