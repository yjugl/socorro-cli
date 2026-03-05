// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::commands::crash_pings::format_frame_location;
use crate::models::bugs::BugsSummary;
use crate::models::crash_pings::{CrashPingStackSummary, CrashPingsSummary};
use crate::models::{CorrelationsSummary, CrashSummary, ModulesMode, SearchResponse, StackFrame};
use std::collections::HashSet;

fn format_function(frame: &StackFrame) -> String {
    if let Some(func) = &frame.function {
        func.clone()
    } else {
        let mut parts = Vec::new();
        if let Some(offset) = &frame.offset {
            parts.push(offset.clone());
        }
        if let Some(module) = &frame.module {
            parts.push(format!("({})", module));
        }
        if parts.is_empty() {
            "???".to_string()
        } else {
            parts.join(" ")
        }
    }
}

pub fn format_crash(summary: &CrashSummary, modules_mode: ModulesMode) -> String {
    let mut output = String::new();

    output.push_str(&format!("CRASH {}\n", summary.crash_id));
    output.push_str(&format!("sig: {}\n", summary.signature));

    if let Some(reason) = &summary.reason {
        let addr_str = summary.address.as_deref().unwrap_or("");
        let addr_desc = if addr_str == "0x0" || addr_str == "0" {
            " (null ptr)"
        } else {
            ""
        };

        if !addr_str.is_empty() {
            output.push_str(&format!("reason: {} @ {}{}\n", reason, addr_str, addr_desc));
        } else {
            output.push_str(&format!("reason: {}\n", reason));
        }
    }

    if let Some(moz_reason) = &summary.moz_crash_reason {
        output.push_str(&format!("moz_reason: {}\n", moz_reason));
    }

    if let Some(abort) = &summary.abort_message {
        output.push_str(&format!("abort: {}\n", abort));
    }

    let device_info = match (&summary.android_model, &summary.android_version) {
        (Some(model), Some(version)) => format!(", {} {}", model, version),
        (Some(model), None) => format!(", {}", model),
        _ => String::new(),
    };

    output.push_str(&format!(
        "product: {} {} ({}{})\n",
        summary.product, summary.version, summary.platform, device_info
    ));

    if let Some(build_id) = &summary.build_id {
        output.push_str(&format!("build: {}\n", build_id));
    }

    if let Some(channel) = &summary.release_channel {
        output.push_str(&format!("channel: {}\n", channel));
    }

    if !summary.all_threads.is_empty() {
        output.push('\n');
        for thread in &summary.all_threads {
            let thread_name = thread.thread_name.as_deref().unwrap_or("unknown");
            let crash_marker = if thread.is_crashing {
                " [CRASHING]"
            } else {
                ""
            };
            output.push_str(&format!(
                "stack[thread {}:{}{}]:\n",
                thread.thread_index, thread_name, crash_marker
            ));

            for frame in &thread.frames {
                let func = format_function(frame);
                let location = match (&frame.file, frame.line) {
                    (Some(file), Some(line)) => format!(" @ {}:{}", file, line),
                    (Some(file), None) => format!(" @ {}", file),
                    _ => String::new(),
                };
                output.push_str(&format!("  #{} {}{}\n", frame.frame, func, location));
            }
            output.push('\n');
        }
    } else if !summary.frames.is_empty() {
        output.push('\n');
        let thread_name = summary.crashing_thread_name.as_deref().unwrap_or("unknown");
        output.push_str(&format!("stack[{}]:\n", thread_name));

        for frame in &summary.frames {
            let func = format_function(frame);
            let location = match (&frame.file, frame.line) {
                (Some(file), Some(line)) => format!(" @ {}:{}", file, line),
                (Some(file), None) => format!(" @ {}", file),
                _ => String::new(),
            };
            output.push_str(&format!("  #{} {}{}\n", frame.frame, func, location));
        }
    }

    output.push_str(&format_modules(summary, modules_mode));

    output
}

fn format_modules(summary: &CrashSummary, mode: ModulesMode) -> String {
    if mode == ModulesMode::None || summary.modules.is_empty() {
        return String::new();
    }

    let modules: Vec<_> = match mode {
        ModulesMode::Stack => {
            let mut module_names: HashSet<&str> = HashSet::new();
            if !summary.all_threads.is_empty() {
                for thread in &summary.all_threads {
                    for frame in &thread.frames {
                        if let Some(m) = &frame.module {
                            module_names.insert(m);
                        }
                    }
                }
            } else {
                for frame in &summary.frames {
                    if let Some(m) = &frame.module {
                        module_names.insert(m);
                    }
                }
            }
            summary
                .modules
                .iter()
                .filter(|m| module_names.contains(m.filename.as_str()))
                .collect()
        }
        ModulesMode::Full => summary.modules.iter().collect(),
        ModulesMode::None => unreachable!(),
    };

    if modules.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("\nmodules:\n");
    for m in &modules {
        let version = m.version.as_deref().unwrap_or("?");
        let debug_file = m.debug_file.as_deref().unwrap_or("?");
        let debug_id = m.debug_id.as_deref().unwrap_or("?");
        let code_id = m.code_id.as_deref().unwrap_or("?");
        out.push_str(&format!(
            "  {} {} | {} | {} | {}\n",
            m.filename, version, debug_file, debug_id, code_id
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CrashHit, CrashSummary, FacetBucket, ModuleInfo, ModulesMode, ThreadSummary,
    };
    use std::collections::HashMap;

    fn sample_crash_summary() -> CrashSummary {
        CrashSummary {
            crash_id: "247653e8-7a18-4836-97d1-42a720260120".to_string(),
            signature: "mozilla::AudioDecoderInputTrack::EnsureTimeStretcher".to_string(),
            reason: Some("SIGSEGV".to_string()),
            address: Some("0x0".to_string()),
            moz_crash_reason: Some("MOZ_RELEASE_ASSERT(mTimeStretcher->Init())".to_string()),
            abort_message: None,
            product: "Fenix".to_string(),
            version: "147.0.1".to_string(),
            build_id: Some("20240115103000".to_string()),
            release_channel: Some("release".to_string()),
            platform: "Android 36".to_string(),
            android_version: Some("36".to_string()),
            android_model: Some("SM-S918B".to_string()),
            crashing_thread_name: Some("GraphRunner".to_string()),
            frames: vec![StackFrame {
                frame: 0,
                function: Some("EnsureTimeStretcher".to_string()),
                file: Some("AudioDecoderInputTrack.cpp".to_string()),
                line: Some(624),
                module: None,
                offset: None,
            }],
            all_threads: vec![],
            modules: vec![],
        }
    }

    fn sample_crash_summary_with_modules() -> CrashSummary {
        CrashSummary {
            crash_id: "test-modules".to_string(),
            signature: "TestSig".to_string(),
            reason: None,
            address: None,
            moz_crash_reason: None,
            abort_message: None,
            product: "Firefox".to_string(),
            version: "148.0".to_string(),
            build_id: None,
            release_channel: None,
            platform: "Windows".to_string(),
            android_version: None,
            android_model: None,
            crashing_thread_name: Some("main".to_string()),
            frames: vec![
                StackFrame {
                    frame: 0,
                    function: Some("func_a".to_string()),
                    file: None,
                    line: None,
                    module: Some("xul.dll".to_string()),
                    offset: None,
                },
                StackFrame {
                    frame: 1,
                    function: Some("func_b".to_string()),
                    file: None,
                    line: None,
                    module: Some("ntdll.dll".to_string()),
                    offset: None,
                },
            ],
            all_threads: vec![],
            modules: vec![
                ModuleInfo {
                    filename: "xul.dll".to_string(),
                    debug_file: Some("xul.pdb".to_string()),
                    debug_id: Some("F51BCD2A".to_string()),
                    code_id: Some("69934c4b".to_string()),
                    version: Some("148.0.0.3".to_string()),
                },
                ModuleInfo {
                    filename: "ntdll.dll".to_string(),
                    debug_file: Some("ntdll.pdb".to_string()),
                    debug_id: Some("180BF1B9".to_string()),
                    code_id: Some("7ec9c15d".to_string()),
                    version: Some("6.2.19041.6456".to_string()),
                },
                ModuleInfo {
                    filename: "mozglue.dll".to_string(),
                    debug_file: Some("mozglue.pdb".to_string()),
                    debug_id: Some("AABBCCDD".to_string()),
                    code_id: Some("abc123".to_string()),
                    version: Some("148.0".to_string()),
                },
            ],
        }
    }

    #[test]
    fn test_format_crash_header() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("CRASH 247653e8-7a18-4836-97d1-42a720260120"));
        assert!(output.contains("sig: mozilla::AudioDecoderInputTrack::EnsureTimeStretcher"));
    }

    #[test]
    fn test_format_crash_reason_with_null_ptr() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("reason: SIGSEGV @ 0x0 (null ptr)"));
    }

    #[test]
    fn test_format_crash_moz_reason() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("moz_reason: MOZ_RELEASE_ASSERT(mTimeStretcher->Init())"));
    }

    #[test]
    fn test_format_crash_product_with_device() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("product: Fenix 147.0.1 (Android 36, SM-S918B 36)"));
    }

    #[test]
    fn test_format_crash_stack_trace() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("stack[GraphRunner]:"));
        assert!(output.contains("#0 EnsureTimeStretcher @ AudioDecoderInputTrack.cpp:624"));
    }

    #[test]
    fn test_format_crash_with_all_threads() {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![
            ThreadSummary {
                thread_index: 0,
                thread_name: Some("MainThread".to_string()),
                frames: vec![],
                is_crashing: false,
            },
            ThreadSummary {
                thread_index: 1,
                thread_name: Some("GraphRunner".to_string()),
                frames: vec![],
                is_crashing: true,
            },
        ];
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("stack[thread 0:MainThread]:"));
        assert!(output.contains("stack[thread 1:GraphRunner [CRASHING]]:"));
    }

    #[test]
    fn test_format_crash_modules_none() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(!output.contains("modules:"));
        assert!(!output.contains("xul.dll"));
    }

    #[test]
    fn test_format_crash_modules_stack() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::Stack);

        assert!(output.contains("modules:"));
        assert!(output.contains("xul.dll 148.0.0.3 | xul.pdb | F51BCD2A | 69934c4b"));
        assert!(output.contains("ntdll.dll 6.2.19041.6456 | ntdll.pdb | 180BF1B9 | 7ec9c15d"));
        // mozglue.dll is NOT in any stack frame, so should be excluded
        assert!(!output.contains("mozglue.dll"));
    }

    #[test]
    fn test_format_crash_modules_full() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::Full);

        assert!(output.contains("modules:"));
        assert!(output.contains("xul.dll 148.0.0.3 | xul.pdb | F51BCD2A | 69934c4b"));
        assert!(output.contains("ntdll.dll 6.2.19041.6456 | ntdll.pdb | 180BF1B9 | 7ec9c15d"));
        // mozglue.dll IS included in full mode
        assert!(output.contains("mozglue.dll 148.0 | mozglue.pdb | AABBCCDD | abc123"));
    }

    #[test]
    fn test_format_crash_modules_stack_with_all_threads() {
        let mut summary = sample_crash_summary_with_modules();
        summary.frames = vec![];
        summary.all_threads = vec![
            ThreadSummary {
                thread_index: 0,
                thread_name: Some("Main".to_string()),
                frames: vec![StackFrame {
                    frame: 0,
                    function: Some("main".to_string()),
                    file: None,
                    line: None,
                    module: Some("mozglue.dll".to_string()),
                    offset: None,
                }],
                is_crashing: false,
            },
            ThreadSummary {
                thread_index: 1,
                thread_name: Some("Worker".to_string()),
                frames: vec![StackFrame {
                    frame: 0,
                    function: Some("work".to_string()),
                    file: None,
                    line: None,
                    module: Some("xul.dll".to_string()),
                    offset: None,
                }],
                is_crashing: true,
            },
        ];
        let output = format_crash(&summary, ModulesMode::Stack);

        // Both mozglue.dll and xul.dll are in threads, so both should appear
        assert!(output.contains("mozglue.dll"));
        assert!(output.contains("xul.dll"));
        // ntdll.dll is NOT in any thread frame
        assert!(!output.contains("ntdll.dll"));
    }

    #[test]
    fn test_format_crash_modules_empty_modules_list() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::Full);

        // No modules section when modules list is empty
        assert!(!output.contains("modules:"));
    }

    #[test]
    fn test_format_search_basic() {
        let response = SearchResponse {
            total: 42,
            hits: vec![CrashHit {
                uuid: "247653e8-7a18-4836-97d1-42a720260120".to_string(),
                date: "2024-01-15".to_string(),
                signature: "mozilla::SomeFunction".to_string(),
                product: "Firefox".to_string(),
                version: "120.0".to_string(),
                platform: Some("Windows".to_string()),
                build_id: Some("20240115103000".to_string()),
                release_channel: Some("release".to_string()),
                platform_version: Some("10.0.19045".to_string()),
            }],
            facets: HashMap::new(),
        };
        let output = format_search(&response);

        assert!(output.contains("FOUND 42 crashes"));
        assert!(output.contains("247653e8"));
        assert!(output.contains("2024-01-15"));
        assert!(output.contains("Firefox 120.0"));
        assert!(output.contains("Windows 10.0.19045"));
        assert!(output.contains("mozilla::SomeFunction"));
    }

    #[test]
    fn test_format_search_with_facets() {
        let mut facets = HashMap::new();
        facets.insert(
            "version".to_string(),
            vec![
                FacetBucket {
                    term: "120.0".to_string(),
                    count: 50,
                },
                FacetBucket {
                    term: "119.0".to_string(),
                    count: 30,
                },
            ],
        );
        let response = SearchResponse {
            total: 80,
            hits: vec![],
            facets,
        };
        let output = format_search(&response);

        assert!(output.contains("AGGREGATIONS:"));
        assert!(output.contains("version:"));
        assert!(output.contains("120.0 (50)"));
        assert!(output.contains("119.0 (30)"));
    }

    #[test]
    fn test_format_function_with_function_name() {
        let frame = StackFrame {
            frame: 0,
            function: Some("my_function".to_string()),
            file: None,
            line: None,
            module: None,
            offset: None,
        };
        assert_eq!(format_function(&frame), "my_function");
    }

    #[test]
    fn test_format_function_without_function_name() {
        let frame = StackFrame {
            frame: 0,
            function: None,
            file: None,
            line: None,
            module: Some("libfoo.so".to_string()),
            offset: Some("0x1234".to_string()),
        };
        assert_eq!(format_function(&frame), "0x1234 (libfoo.so)");
    }

    #[test]
    fn test_format_function_unknown() {
        let frame = StackFrame {
            frame: 0,
            function: None,
            file: None,
            line: None,
            module: None,
            offset: None,
        };
        assert_eq!(format_function(&frame), "???");
    }

    use crate::models::bugs::{BugGroup, BugsSummary};
    use crate::models::{CorrelationItem, CorrelationItemPrior, CorrelationsSummary};

    #[test]
    fn test_format_bugs_with_results() {
        let summary = BugsSummary {
            bugs: vec![
                BugGroup {
                    bug_id: 888888,
                    signatures: vec!["OOM | small".to_string()],
                },
                BugGroup {
                    bug_id: 999999,
                    signatures: vec!["OOM | large".to_string(), "OOM | small".to_string()],
                },
            ],
        };
        let output = format_bugs(&summary);
        assert!(output.contains("bug 888888\n"));
        assert!(output.contains("  OOM | small\n"));
        assert!(output.contains("bug 999999\n"));
        assert!(output.contains("  OOM | large\n"));
    }

    #[test]
    fn test_format_bugs_empty() {
        let summary = BugsSummary { bugs: vec![] };
        let output = format_bugs(&summary);
        assert!(output.contains("No bugs found."));
    }

    fn sample_correlations_summary() -> CorrelationsSummary {
        CorrelationsSummary {
            signature: "TestSig".to_string(),
            channel: "release".to_string(),
            date: "2026-02-13".to_string(),
            sig_count: 220.0,
            ref_count: 79268,
            items: vec![
                CorrelationItem {
                    label: "Module \"cscapi.dll\" = true".to_string(),
                    sig_pct: 100.0,
                    ref_pct: 24.51,
                    prior: None,
                },
                CorrelationItem {
                    label: "startup_crash = null".to_string(),
                    sig_pct: 29.55,
                    ref_pct: 1.16,
                    prior: Some(CorrelationItemPrior {
                        label: "process_type = parent".to_string(),
                        sig_pct: 50.91,
                        ref_pct: 4.58,
                    }),
                },
            ],
        }
    }

    #[test]
    fn test_format_correlations_header() {
        let summary = sample_correlations_summary();
        let output = format_correlations(&summary);
        assert!(output.contains("CORRELATIONS for \"TestSig\" (release, data from 2026-02-13)"));
        assert!(output.contains("sig_count: 220, ref_count: 79268"));
    }

    #[test]
    fn test_format_correlations_items() {
        let summary = sample_correlations_summary();
        let output = format_correlations(&summary);
        assert!(output.contains("(100.00% vs 24.51% overall) Module \"cscapi.dll\" = true"));
    }

    #[test]
    fn test_format_correlations_with_prior() {
        let summary = sample_correlations_summary();
        let output = format_correlations(&summary);
        assert!(output.contains("(029.55% vs 01.16% overall) startup_crash = null [50.91% vs 04.58% if process_type = parent]"));
    }

    #[test]
    fn test_format_correlations_empty() {
        let summary = CorrelationsSummary {
            signature: "EmptySig".to_string(),
            channel: "release".to_string(),
            date: "2026-02-13".to_string(),
            sig_count: 0.0,
            ref_count: 79268,
            items: vec![],
        };
        let output = format_correlations(&summary);
        assert!(output.contains("No correlations found."));
    }
}

pub fn format_correlations(summary: &CorrelationsSummary) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "CORRELATIONS for \"{}\" ({}, data from {})\n",
        summary.signature, summary.channel, summary.date
    ));
    output.push_str(&format!(
        "sig_count: {}, ref_count: {}\n\n",
        summary.sig_count as u64, summary.ref_count
    ));

    if summary.items.is_empty() {
        output.push_str("No correlations found.\n");
    } else {
        for item in &summary.items {
            let prior_str = if let Some(prior) = &item.prior {
                format!(
                    " [{:05.2}% vs {:05.2}% if {}]",
                    prior.sig_pct, prior.ref_pct, prior.label
                )
            } else {
                String::new()
            };
            output.push_str(&format!(
                "({:06.2}% vs {:05.2}% overall) {}{}\n",
                item.sig_pct, item.ref_pct, item.label, prior_str
            ));
        }
    }

    output
}

pub fn format_crash_pings(summary: &CrashPingsSummary) -> String {
    let mut output = String::new();

    let date_str = if summary.date_from == summary.date_to {
        summary.date_from.clone()
    } else {
        format!("{}..{}", summary.date_from, summary.date_to)
    };
    let filter_str = if let Some(ref sig) = summary.signature_filter {
        format!(": \"{}\" ({} pings)", sig, summary.filtered_total)
    } else {
        format!(" ({} pings, sampled)", summary.total)
    };
    output.push_str(&format!("CRASH PINGS {}{}\n\n", date_str, filter_str));

    if summary.facet_name != "signature" || summary.signature_filter.is_some() {
        output.push_str(&format!("{}:\n", summary.facet_name));
    }

    if summary.items.is_empty() {
        output.push_str("  (no matching pings)\n");
    } else {
        for item in &summary.items {
            output.push_str(&format!(
                "  {} ({}, {:.2}%)\n",
                item.label, item.count, item.percentage
            ));
            if !item.example_ids.is_empty() {
                output.push_str(&format!("    e.g. {}\n", item.example_ids.join(", ")));
            }
        }
    }

    output
}

pub fn format_crash_ping_stack(summary: &CrashPingStackSummary) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "CRASH PING {} ({})\n",
        summary.crash_id, summary.date
    ));

    if summary.frames.is_empty() {
        if summary.java_exception.is_some() {
            output.push_str("\njava_exception:\n");
            if let Some(ref exc) = summary.java_exception {
                output.push_str(&format!("  {}\n", exc));
            }
        } else {
            output.push_str("\nNo stack trace available.\n");
        }
    } else {
        output.push_str("\nstack:\n");
        for (i, frame) in summary.frames.iter().enumerate() {
            output.push_str(&format!("  #{} {}\n", i, format_frame_location(frame)));
        }
    }

    output
}

pub fn format_bugs(summary: &BugsSummary) -> String {
    let mut output = String::new();

    if summary.bugs.is_empty() {
        output.push_str("No bugs found.\n");
    } else {
        for group in &summary.bugs {
            output.push_str(&format!("bug {}\n", group.bug_id));
            for sig in &group.signatures {
                output.push_str(&format!("  {}\n", sig));
            }
        }
    }

    output
}

pub fn format_search(response: &SearchResponse) -> String {
    let mut output = String::new();

    output.push_str(&format!("FOUND {} crashes\n\n", response.total));

    for hit in &response.hits {
        let platform = match (&hit.platform, &hit.platform_version) {
            (Some(p), Some(v)) => format!("{} {}", p, v),
            (Some(p), None) => p.clone(),
            (None, Some(v)) => v.clone(),
            (None, None) => "?".to_string(),
        };
        let channel = hit.release_channel.as_deref().unwrap_or("?");
        let build = hit.build_id.as_deref().unwrap_or("?");
        output.push_str(&format!(
            "{} | {} | {} {} | {} | {} | {} | {}\n",
            hit.uuid, hit.date, hit.product, hit.version, platform, channel, build, hit.signature
        ));
    }

    if !response.facets.is_empty() {
        output.push_str("\nAGGREGATIONS:\n");
        for (field, buckets) in &response.facets {
            output.push_str(&format!("\n{}:\n", field));
            for bucket in buckets {
                output.push_str(&format!("  {} ({})\n", bucket.term, bucket.count));
            }
        }
    }

    output
}
