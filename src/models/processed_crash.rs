// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::annotations::{
    AsyncShutdownTimeout, deserialize_optional_bool, deserialize_optional_u64,
};
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

    // Annotations rendered unconditionally.
    #[serde(default)]
    pub report_type: Option<String>,
    #[serde(default)]
    pub process_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_u64")]
    pub uptime: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_bool")]
    pub startup_crash: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_u64")]
    pub thread_count: Option<u64>,

    // Annotations rendered on request. `async_shutdown_timeout` is a JSON
    // document embedded in a string; it is kept raw here and parsed in
    // `to_summary()`.
    #[serde(default)]
    pub async_shutdown_timeout: Option<String>,
    #[serde(default)]
    pub shutdown_progress: Option<String>,
    #[serde(default)]
    pub shutdown_reason: Option<String>,
    #[serde(default)]
    pub xpcom_spin_event_loop_stack: Option<String>,
    #[serde(default)]
    pub app_notes: Option<String>,
    #[serde(default)]
    pub last_error_value: Option<String>,
    #[serde(default)]
    pub crash_inconsistencies: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub topmost_filenames: Option<String>,
    #[serde(default)]
    pub modules_in_stack: Option<String>,
    #[serde(default)]
    pub proto_signature: Option<String>,
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

#[derive(Debug, Default)]
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

    // Annotations shown unconditionally.
    pub report_type: Option<String>,
    pub process_type: Option<String>,
    pub uptime: Option<u64>,
    pub startup_crash: Option<bool>,
    pub thread_count: Option<u64>,

    // Annotations shown on request. Always populated here; whether they are
    // rendered is the formatter's decision.
    pub async_shutdown_timeout: Option<AsyncShutdownTimeout>,
    pub shutdown_progress: Option<String>,
    pub shutdown_reason: Option<String>,
    pub xpcom_spin_event_loop_stack: Option<String>,
    pub app_notes: Option<String>,
    pub last_error_value: Option<String>,
    /// One display string per entry of `crash_inconsistencies`; empty when the
    /// annotation is absent or its list is empty (the common case).
    pub crash_inconsistencies: Vec<String>,
    pub topmost_filenames: Option<String>,
    pub modules_in_stack: Option<String>,
    pub proto_signature: Option<String>,
}

impl ProcessedCrash {
    /// Convert the raw API response into the display-relevant subset: crash
    /// identity and metadata, the crashing thread's frames capped at `depth`
    /// (plus every thread when `all_threads` is set), the module list, and the
    /// crash annotations. Annotations are always populated; formatters decide
    /// which of them to render.
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

        let crash_inconsistencies: Vec<String> = self
            .crash_inconsistencies
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect();

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
            report_type: self.report_type.clone(),
            process_type: self.process_type.clone(),
            uptime: self.uptime,
            startup_crash: self.startup_crash,
            thread_count: self.thread_count,
            async_shutdown_timeout: self
                .async_shutdown_timeout
                .as_deref()
                .map(AsyncShutdownTimeout::parse),
            shutdown_progress: self.shutdown_progress.clone(),
            shutdown_reason: self.shutdown_reason.clone(),
            xpcom_spin_event_loop_stack: self.xpcom_spin_event_loop_stack.clone(),
            app_notes: self.app_notes.clone(),
            last_error_value: self.last_error_value.clone(),
            crash_inconsistencies,
            topmost_filenames: self.topmost_filenames.clone(),
            modules_in_stack: self.modules_in_stack.clone(),
            proto_signature: self.proto_signature.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Field names, types and value shapes copied from the unauthenticated
    /// `/ProcessedCrash/` response for the shutdown-hang crash
    /// b98bbb81-3ff6-4825-991f-6a0b30260901 (`report_type=hang`,
    /// `process_type=parent`, `uptime=2175`, `thread_count=64`).
    fn annotated_crash_json() -> &'static str {
        r#"{
            "uuid": "b98bbb81-3ff6-4825-991f-6a0b30260901",
            "signature": "shutdownhang | mozilla::SpinEventLoopUntil",
            "report_type": "hang",
            "process_type": "parent",
            "uptime": 2175,
            "startup_crash": false,
            "thread_count": 64,
            "async_shutdown_timeout": "{\"phase\":\"profile-before-change\",\"conditions\":[{\"name\":\"ServiceWorkerRegistrar: Flushing data\",\"state\":{\"saveDataRunnableDispatched\":false,\"shuttingDown\":false},\"filename\":\"..\\\\..\\\\checkouts\\\\gecko\\\\dom\\\\serviceworkers\\\\ServiceWorkerRegistrar.cpp\",\"lineNumber\":1566,\"stack\":\"ServiceWorkerRegistrar: Flushing data\"},{\"name\":\"ASRouterStorage: flush pending writes\",\"state\":{\"pending\":1},\"filename\":\"resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs\",\"lineNumber\":50,\"stack\":[\"resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs:createStorage:50\"]},{\"name\":\"ShieldRecipeClient: Cleaning up\",\"state\":\"(none)\",\"filename\":\"resource://normandy/lib/CleanupManager.sys.mjs\",\"lineNumber\":39,\"stack\":[\"resource://normandy/lib/CleanupManager.sys.mjs:cleanup:39\"]}]}",
            "shutdown_progress": "profile-before-change",
            "shutdown_reason": "AppClose",
            "xpcom_spin_event_loop_stack": "default: AsyncShutdown Spinner for profile-before-change",
            "app_notes": "FP(D00-L1000-W0000100-T1) DWrite? DWrite+ WR! WR+",
            "last_error_value": "ERROR_SUCCESS",
            "crash_inconsistencies": ["crashing thread mismatch"],
            "topmost_filenames": "git:github.com/mozilla-firefox/firefox:mfbt/Assertions.h:9b794146",
            "modules_in_stack": "firefox.exe/77DFC624;ntdll.dll/3D1FD1A7",
            "proto_signature": "MOZ_Crash | Abort | NS_PrintStackTrace"
        }"#
    }

    #[test]
    fn test_deserialize_always_on_annotations() {
        let crash: ProcessedCrash = serde_json::from_str(annotated_crash_json()).unwrap();
        assert_eq!(crash.report_type, Some("hang".to_string()));
        assert_eq!(crash.process_type, Some("parent".to_string()));
        assert_eq!(crash.uptime, Some(2175));
        assert_eq!(crash.startup_crash, Some(false));
        assert_eq!(crash.thread_count, Some(64));
    }

    #[test]
    fn test_deserialize_opt_in_annotations() {
        let crash: ProcessedCrash = serde_json::from_str(annotated_crash_json()).unwrap();
        assert_eq!(
            crash.shutdown_progress,
            Some("profile-before-change".to_string())
        );
        assert_eq!(crash.shutdown_reason, Some("AppClose".to_string()));
        assert_eq!(
            crash.xpcom_spin_event_loop_stack,
            Some("default: AsyncShutdown Spinner for profile-before-change".to_string())
        );
        assert!(crash.app_notes.as_deref().unwrap().contains("DWrite?"));
        assert_eq!(crash.last_error_value, Some("ERROR_SUCCESS".to_string()));
        assert_eq!(crash.crash_inconsistencies.as_ref().unwrap().len(), 1);
        assert!(
            crash
                .topmost_filenames
                .as_deref()
                .unwrap()
                .contains("Assertions.h")
        );
        assert!(
            crash
                .modules_in_stack
                .as_deref()
                .unwrap()
                .contains("firefox.exe")
        );
        assert_eq!(
            crash.proto_signature,
            Some("MOZ_Crash | Abort | NS_PrintStackTrace".to_string())
        );
        assert!(crash.async_shutdown_timeout.is_some());
    }

    #[test]
    fn test_to_summary_annotations() {
        let crash: ProcessedCrash = serde_json::from_str(annotated_crash_json()).unwrap();
        let summary = crash.to_summary(10, false);

        assert_eq!(summary.report_type, Some("hang".to_string()));
        assert_eq!(summary.process_type, Some("parent".to_string()));
        assert_eq!(summary.uptime, Some(2175));
        assert_eq!(summary.startup_crash, Some(false));
        assert_eq!(summary.thread_count, Some(64));
        assert_eq!(
            summary.shutdown_progress,
            Some("profile-before-change".to_string())
        );
        assert_eq!(summary.shutdown_reason, Some("AppClose".to_string()));
        assert_eq!(summary.last_error_value, Some("ERROR_SUCCESS".to_string()));
        assert_eq!(
            summary.crash_inconsistencies,
            vec!["crashing thread mismatch"]
        );
        assert!(summary.proto_signature.is_some());
        assert!(summary.topmost_filenames.is_some());
        assert!(summary.modules_in_stack.is_some());
        assert!(summary.app_notes.is_some());
        assert!(summary.xpcom_spin_event_loop_stack.is_some());
    }

    #[test]
    fn test_to_summary_parses_async_shutdown_timeout() {
        let crash: ProcessedCrash = serde_json::from_str(annotated_crash_json()).unwrap();
        let summary = crash.to_summary(10, false);

        match summary.async_shutdown_timeout.as_ref().unwrap() {
            AsyncShutdownTimeout::Parsed(data) => {
                assert_eq!(data.phase, "profile-before-change");
                assert_eq!(data.conditions.len(), 3);
                assert!(
                    data.conditions[0]
                        .filename
                        .as_deref()
                        .unwrap()
                        .ends_with("ServiceWorkerRegistrar.cpp"),
                    "unexpected filename: {:?}",
                    data.conditions[0].filename
                );
                assert_eq!(data.conditions[0].line_number, Some(1566));
                // The third condition's `state` is the string "(none)", not an
                // object; it must not break parsing of the blob.
                assert_eq!(
                    data.conditions[2].state_display(),
                    Some("(none)".to_string())
                );
            }
            AsyncShutdownTimeout::Raw(raw) => panic!("expected a parsed blob, got raw: {}", raw),
        }
    }

    #[test]
    fn test_to_summary_async_shutdown_timeout_malformed_kept_verbatim() {
        let json = r#"{"uuid": "abc", "async_shutdown_timeout": "phase: profile-before-change"}"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        let summary = crash.to_summary(10, false);

        match summary.async_shutdown_timeout.as_ref().unwrap() {
            AsyncShutdownTimeout::Raw(raw) => assert_eq!(raw, "phase: profile-before-change"),
            AsyncShutdownTimeout::Parsed(data) => panic!("unexpectedly parsed: {:?}", data),
        }
    }

    #[test]
    fn test_annotations_absent_when_api_omits_them() {
        // The sample crash has none of the annotation keys: every value must be
        // absent rather than defaulted to a misleading zero/false.
        let crash: ProcessedCrash = serde_json::from_str(sample_crash_json()).unwrap();
        assert_eq!(crash.report_type, None);
        assert_eq!(crash.process_type, None);
        assert_eq!(crash.uptime, None);
        assert_eq!(crash.startup_crash, None);
        assert_eq!(crash.thread_count, None);
        assert_eq!(crash.async_shutdown_timeout, None);
        assert_eq!(crash.crash_inconsistencies, None);

        let summary = crash.to_summary(10, false);
        assert_eq!(summary.report_type, None);
        assert_eq!(summary.process_type, None);
        assert_eq!(summary.uptime, None);
        assert_eq!(summary.startup_crash, None);
        assert_eq!(summary.thread_count, None);
        assert!(summary.async_shutdown_timeout.is_none());
        assert!(summary.shutdown_progress.is_none());
        assert!(summary.shutdown_reason.is_none());
        assert!(summary.xpcom_spin_event_loop_stack.is_none());
        assert!(summary.app_notes.is_none());
        assert!(summary.last_error_value.is_none());
        assert!(summary.topmost_filenames.is_none());
        assert!(summary.modules_in_stack.is_none());
        assert!(summary.proto_signature.is_none());
        assert!(summary.crash_inconsistencies.is_empty());
    }

    #[test]
    fn test_annotations_tolerate_string_scalars() {
        // Defensive: a numeric or boolean annotation arriving as a string must
        // not fail the whole crash deserialization.
        let json =
            r#"{"uuid": "abc", "uptime": "2175", "thread_count": "64", "startup_crash": "1"}"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        assert_eq!(crash.uptime, Some(2175));
        assert_eq!(crash.thread_count, Some(64));
        assert_eq!(crash.startup_crash, Some(true));

        // An unparseable value degrades to absent, not to an error.
        let json = r#"{"uuid": "abc", "uptime": "unknown", "startup_crash": []}"#;
        let crash: ProcessedCrash = serde_json::from_str(json).unwrap();
        assert_eq!(crash.uptime, None);
        assert_eq!(crash.startup_crash, None);
    }

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
