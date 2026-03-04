// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::{ModuleInfo, StackFrame, common::deserialize_string_or_number};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessedCrash {
    pub uuid: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub os_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub build: Option<String>,
    #[serde(default)]
    pub release_channel: Option<String>,
    #[serde(default)]
    pub os_version: Option<String>,

    #[serde(default)]
    pub crash_info: Option<CrashInfo>,
    #[serde(default)]
    pub moz_crash_reason: Option<String>,
    #[serde(default)]
    pub abort_message: Option<String>,

    #[serde(default)]
    pub android_model: Option<String>,
    #[serde(default)]
    pub android_version: Option<String>,

    #[serde(default)]
    pub crashing_thread: Option<usize>,
    #[serde(default)]
    pub threads: Option<Vec<Thread>>,
    #[serde(default)]
    pub json_dump: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashInfo {
    #[serde(rename = "type")]
    pub crash_type: Option<String>,
    pub address: Option<String>,
    pub crashing_thread: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Thread {
    pub thread: Option<usize>,
    pub thread_name: Option<String>,
    pub frames: Vec<StackFrame>,
}

#[derive(Debug, Clone)]
pub struct ThreadSummary {
    pub thread_index: usize,
    pub thread_name: Option<String>,
    pub frames: Vec<StackFrame>,
    pub is_crashing: bool,
}

#[derive(Debug)]
pub struct CrashSummary {
    pub crash_id: String,
    pub signature: String,
    pub reason: Option<String>,
    pub address: Option<String>,
    pub moz_crash_reason: Option<String>,
    pub abort_message: Option<String>,

    pub product: String,
    pub version: String,
    pub build_id: Option<String>,
    pub release_channel: Option<String>,
    pub platform: String,

    pub android_version: Option<String>,
    pub android_model: Option<String>,

    pub crashing_thread_name: Option<String>,
    pub frames: Vec<StackFrame>,
    pub all_threads: Vec<ThreadSummary>,
    pub modules: Vec<ModuleInfo>,
}

impl ProcessedCrash {
    pub fn to_summary(&self, depth: usize, all_threads: bool) -> CrashSummary {
        let crashing_thread_idx = self
            .crashing_thread
            .or_else(|| self.crash_info.as_ref().and_then(|ci| ci.crashing_thread))
            .or_else(|| {
                self.json_dump.as_ref().and_then(|jd| {
                    jd.get("crashing_thread")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize)
                })
            });

        let json_dump_threads: Option<Vec<Thread>> = self
            .json_dump
            .as_ref()
            .and_then(|jd| jd.get("threads"))
            .and_then(|t| serde_json::from_value(t.clone()).ok());

        let threads_data = self.threads.as_ref().or(json_dump_threads.as_ref());

        let (thread_name, frames, thread_summaries) = if let Some(threads) = threads_data {
            let mut all_thread_summaries = Vec::new();

            if all_threads {
                for (idx, thread) in threads.iter().enumerate() {
                    let frames: Vec<StackFrame> =
                        thread.frames.iter().take(depth).cloned().collect();
                    all_thread_summaries.push(ThreadSummary {
                        thread_index: idx,
                        thread_name: thread.thread_name.clone(),
                        frames,
                        is_crashing: Some(idx) == crashing_thread_idx,
                    });
                }
            }

            if let Some(idx) = crashing_thread_idx {
                if let Some(thread) = threads.get(idx) {
                    let frames: Vec<StackFrame> =
                        thread.frames.iter().take(depth).cloned().collect();
                    (thread.thread_name.clone(), frames, all_thread_summaries)
                } else {
                    (None, Vec::new(), all_thread_summaries)
                }
            } else {
                (None, Vec::new(), all_thread_summaries)
            }
        } else {
            (None, Vec::new(), Vec::new())
        };

        let modules: Vec<ModuleInfo> = self
            .json_dump
            .as_ref()
            .and_then(|jd| jd.get("modules"))
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let json_dump_crash_info: Option<CrashInfo> = self
            .json_dump
            .as_ref()
            .and_then(|jd| jd.get("crash_info"))
            .and_then(|ci| serde_json::from_value(ci.clone()).ok());

        let crash_info = self.crash_info.as_ref().or(json_dump_crash_info.as_ref());

        CrashSummary {
            crash_id: self.uuid.clone(),
            signature: self
                .signature
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            reason: crash_info.and_then(|ci| ci.crash_type.clone()),
            address: crash_info.and_then(|ci| ci.address.clone()),
            moz_crash_reason: self.moz_crash_reason.clone(),
            abort_message: self.abort_message.clone(),
            product: self
                .product
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            version: self
                .version
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            build_id: self.build.clone(),
            release_channel: self.release_channel.clone(),
            platform: format!(
                "{}{}",
                self.os_name.as_deref().unwrap_or("Unknown"),
                self.os_version
                    .as_ref()
                    .map(|v| format!(" {}", v))
                    .unwrap_or_default()
            ),
            android_version: self.android_version.clone(),
            android_model: self.android_model.clone(),
            crashing_thread_name: thread_name,
            frames,
            all_threads: thread_summaries,
            modules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_crash_json() -> &'static str {
        r#"{
            "uuid": "247653e8-7a18-4836-97d1-42a720260120",
            "signature": "mozilla::AudioDecoderInputTrack::EnsureTimeStretcher",
            "product": "Fenix",
            "version": "147.0.1",
            "os_name": "Android",
            "os_version": "36",
            "crashing_thread": 1,
            "moz_crash_reason": "MOZ_RELEASE_ASSERT(mTimeStretcher->Init())",
            "crash_info": {
                "type": "SIGSEGV",
                "address": "0x0",
                "crashing_thread": 1
            },
            "json_dump": {
                "modules": [
                    {
                        "filename": "xul.dll",
                        "debug_file": "xul.pdb",
                        "debug_id": "F51BCD2A59EB2A194C4C44205044422E1",
                        "code_id": "69934c4ba31f000",
                        "version": "148.0.0.3"
                    },
                    {
                        "filename": "ntdll.dll",
                        "debug_file": "ntdll.pdb",
                        "debug_id": "180BF1B90AA75697D0EFEA5E5630AC7E1",
                        "code_id": "7ec9c15d1f8000",
                        "version": "6.2.19041.6456"
                    },
                    {
                        "filename": "mozglue.dll",
                        "debug_file": "mozglue.pdb",
                        "debug_id": "AABBCCDD11223344",
                        "code_id": "abc123",
                        "version": "148.0"
                    }
                ]
            },
            "threads": [
                {
                    "thread": 0,
                    "thread_name": "MainThread",
                    "frames": [
                        {"frame": 0, "function": "main", "file": "main.cpp", "line": 10, "module": "xul.dll"}
                    ]
                },
                {
                    "thread": 1,
                    "thread_name": "GraphRunner",
                    "frames": [
                        {"frame": 0, "function": "EnsureTimeStretcher", "file": "AudioDecoderInputTrack.cpp", "line": 624, "module": "xul.dll"},
                        {"frame": 1, "function": "AppendData", "file": "AudioDecoderInputTrack.cpp", "line": 423, "module": "ntdll.dll"}
                    ]
                }
            ]
        }"#
    }

    #[test]
    fn test_deserialize_processed_crash() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        assert_eq!(crash.uuid, "247653e8-7a18-4836-97d1-42a720260120");
        assert_eq!(
            crash.signature,
            Some("mozilla::AudioDecoderInputTrack::EnsureTimeStretcher".to_string())
        );
        assert_eq!(crash.product, Some("Fenix".to_string()));
        assert_eq!(crash.version, Some("147.0.1".to_string()));
        assert_eq!(crash.crashing_thread, Some(1));
    }

    #[test]
    fn test_to_summary_basic() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.crash_id, "247653e8-7a18-4836-97d1-42a720260120");
        assert_eq!(
            summary.signature,
            "mozilla::AudioDecoderInputTrack::EnsureTimeStretcher"
        );
        assert_eq!(summary.product, "Fenix");
        assert_eq!(summary.version, "147.0.1");
        assert_eq!(summary.reason, Some("SIGSEGV".to_string()));
        assert_eq!(summary.address, Some("0x0".to_string()));
        assert_eq!(
            summary.moz_crash_reason,
            Some("MOZ_RELEASE_ASSERT(mTimeStretcher->Init())".to_string())
        );
    }

    #[test]
    fn test_to_summary_crashing_thread_frames() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(
            summary.crashing_thread_name,
            Some("GraphRunner".to_string())
        );
        assert_eq!(summary.frames.len(), 2);
        assert_eq!(
            summary.frames[0].function,
            Some("EnsureTimeStretcher".to_string())
        );
    }

    #[test]
    fn test_to_summary_depth_limit() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        let summary = crash.to_summary(1, false);

        assert_eq!(summary.frames.len(), 1);
        assert_eq!(
            summary.frames[0].function,
            Some("EnsureTimeStretcher".to_string())
        );
    }

    #[test]
    fn test_to_summary_all_threads() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        let summary = crash.to_summary(10, true);

        assert_eq!(summary.all_threads.len(), 2);
        assert!(!summary.all_threads[0].is_crashing);
        assert!(summary.all_threads[1].is_crashing);
        assert_eq!(
            summary.all_threads[0].thread_name,
            Some("MainThread".to_string())
        );
        assert_eq!(
            summary.all_threads[1].thread_name,
            Some("GraphRunner".to_string())
        );
    }

    #[test]
    fn test_crashing_thread_from_crash_info() {
        // Test fallback to crash_info.crashing_thread when crashing_thread is not set
        let json = r#"{
            "uuid": "test-crash",
            "crash_info": {
                "type": "SIGSEGV",
                "crashing_thread": 0
            },
            "threads": [
                {"thread": 0, "thread_name": "Main", "frames": [{"frame": 0, "function": "foo"}]}
            ]
        }"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.crashing_thread_name, Some("Main".to_string()));
    }

    #[test]
    fn test_crashing_thread_from_json_dump() {
        // Test fallback to json_dump.crashing_thread
        let json = r#"{
            "uuid": "test-crash",
            "json_dump": {
                "crashing_thread": 0,
                "threads": [
                    {"thread": 0, "thread_name": "DumpThread", "frames": [{"frame": 0, "function": "bar"}]}
                ]
            }
        }"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.crashing_thread_name, Some("DumpThread".to_string()));
    }

    #[test]
    fn test_missing_optional_fields() {
        let json = r#"{"uuid": "minimal-crash"}"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.crash_id, "minimal-crash");
        assert_eq!(summary.signature, "Unknown");
        assert_eq!(summary.product, "Unknown");
        assert!(summary.frames.is_empty());
        assert!(summary.modules.is_empty());
    }

    #[test]
    fn test_to_summary_extracts_modules() {
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.modules.len(), 3);
        assert_eq!(summary.modules[0].filename, "xul.dll");
        assert_eq!(summary.modules[0].debug_file, Some("xul.pdb".to_string()));
        assert_eq!(
            summary.modules[0].debug_id,
            Some("F51BCD2A59EB2A194C4C44205044422E1".to_string())
        );
        assert_eq!(
            summary.modules[0].code_id,
            Some("69934c4ba31f000".to_string())
        );
        assert_eq!(summary.modules[0].version, Some("148.0.0.3".to_string()));
    }

    #[test]
    fn test_to_summary_modules_missing_json_dump() {
        let json = r#"{
            "uuid": "no-json-dump",
            "threads": [
                {"thread": 0, "frames": [{"frame": 0, "function": "foo"}]}
            ]
        }"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert!(summary.modules.is_empty());
    }

    #[test]
    fn test_to_summary_modules_missing_modules_key() {
        let json = r#"{
            "uuid": "no-modules",
            "json_dump": {
                "crashing_thread": 0,
                "threads": [
                    {"thread": 0, "frames": [{"frame": 0, "function": "foo"}]}
                ]
            }
        }"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert!(summary.modules.is_empty());
    }

    #[test]
    fn test_to_summary_modules_optional_fields() {
        let json = r#"{
            "uuid": "partial-modules",
            "json_dump": {
                "modules": [
                    {"filename": "bare.dll"}
                ]
            }
        }"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.modules.len(), 1);
        assert_eq!(summary.modules[0].filename, "bare.dll");
        assert!(summary.modules[0].debug_file.is_none());
        assert!(summary.modules[0].debug_id.is_none());
        assert!(summary.modules[0].code_id.is_none());
        assert!(summary.modules[0].version.is_none());
    }
}
