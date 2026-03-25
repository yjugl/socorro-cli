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

pub fn format_bugs(summary: &BugsSummary) -> String {
    let mut output = String::new();

    output.push_str("# Bug Associations\n\n");

    if summary.bugs.is_empty() {
        output.push_str("No bugs found.\n");
    } else {
        output.push_str("| Bug | Signatures |\n");
        output.push_str("|-----|------------|\n");
        for group in &summary.bugs {
            let sigs = group
                .signatures
                .iter()
                .map(|s| format!("`{}`", s))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!(
                "| [{}](https://bugzilla.mozilla.org/show_bug.cgi?id={}) | {} |\n",
                group.bug_id, group.bug_id, sigs
            ));
        }
    }

    output
}

pub fn format_crash(summary: &CrashSummary, modules_mode: ModulesMode) -> String {
    let mut output = String::new();

    output.push_str("# Crash Report\n\n");
    output.push_str(&format!("**Crash ID:** `{}`\n\n", summary.crash_id));
    output.push_str(&format!("**Signature:** `{}`\n\n", summary.signature));

    output.push_str("## Details\n\n");

    if let Some(reason) = &summary.reason {
        let addr_str = summary.address.as_deref().unwrap_or("");
        let addr_desc = if addr_str == "0x0" || addr_str == "0" {
            " (null pointer)"
        } else {
            ""
        };

        if !addr_str.is_empty() {
            output.push_str(&format!(
                "- **Crash Reason:** {} at `{}`{}\n",
                reason, addr_str, addr_desc
            ));
        } else {
            output.push_str(&format!("- **Crash Reason:** {}\n", reason));
        }
    }

    if let Some(moz_reason) = &summary.moz_crash_reason {
        output.push_str(&format!("- **Mozilla Crash Reason:** {}\n", moz_reason));
    }

    if let Some(abort) = &summary.abort_message {
        output.push_str(&format!("- **Abort Message:** {}\n", abort));
    }

    let device_info = match (&summary.android_model, &summary.android_version) {
        (Some(model), Some(version)) => format!(" on {} (Android {})", model, version),
        (Some(model), None) => format!(" on {}", model),
        _ => String::new(),
    };

    output.push_str(&format!(
        "- **Product:** {} {}\n",
        summary.product, summary.version
    ));
    if let Some(build_id) = &summary.build_id {
        output.push_str(&format!("- **Build ID:** {}\n", build_id));
    }
    if let Some(channel) = &summary.release_channel {
        output.push_str(&format!("- **Release Channel:** {}\n", channel));
    }
    output.push_str(&format!(
        "- **Platform:** {}{}\n\n",
        summary.platform, device_info
    ));

    if !summary.all_threads.is_empty() {
        output.push_str("## All Threads\n\n");
        for thread in &summary.all_threads {
            let thread_name = thread.thread_name.as_deref().unwrap_or("unknown");
            let crash_marker = if thread.is_crashing {
                " **[CRASHING]**"
            } else {
                ""
            };
            output.push_str(&format!(
                "### Thread {} ({}){}\n\n",
                thread.thread_index, thread_name, crash_marker
            ));
            output.push_str("```\n");

            for frame in &thread.frames {
                let func = format_function(frame);
                let location = match (&frame.file, frame.line) {
                    (Some(file), Some(line)) => format!(" @ {}:{}", file, line),
                    (Some(file), None) => format!(" @ {}", file),
                    _ => String::new(),
                };
                output.push_str(&format!("#{} {}{}\n", frame.frame, func, location));
            }

            output.push_str("```\n\n");
        }
    } else if !summary.frames.is_empty() {
        let thread_name = summary.crashing_thread_name.as_deref().unwrap_or("unknown");
        output.push_str(&format!("## Stack Trace ({})\n\n", thread_name));
        output.push_str("```\n");

        for frame in &summary.frames {
            let func = format_function(frame);
            let location = match (&frame.file, frame.line) {
                (Some(file), Some(line)) => format!(" @ {}:{}", file, line),
                (Some(file), None) => format!(" @ {}", file),
                _ => String::new(),
            };
            output.push_str(&format!("#{} {}{}\n", frame.frame, func, location));
        }

        output.push_str("```\n");
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
        ModulesMode::ThirdParty => summary
            .modules
            .iter()
            .filter(|m| m.is_third_party())
            .collect(),
        ModulesMode::None => unreachable!(),
    };

    if modules.is_empty() {
        return String::new();
    }

    let show_cert = mode == ModulesMode::ThirdParty;
    let mut out = String::new();
    out.push_str("\n## Modules\n\n");
    if show_cert {
        out.push_str("| Module | Version | Debug File | Debug ID | Code ID | Signed By |\n");
        out.push_str("|--------|---------|------------|----------|--------|----------|\n");
    } else {
        out.push_str("| Module | Version | Debug File | Debug ID | Code ID |\n");
        out.push_str("|--------|---------|------------|----------|--------|\n");
    }
    for m in &modules {
        let version = m.version.as_deref().unwrap_or("?");
        let debug_file = m.debug_file.as_deref().unwrap_or("?");
        let debug_id = m.debug_id.as_deref().unwrap_or("?");
        let code_id = m.code_id.as_deref().unwrap_or("?");
        if show_cert {
            let cert = m.cert_subject.as_deref().unwrap_or("unsigned");
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                m.filename, version, debug_file, debug_id, code_id, cert
            ));
        } else {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                m.filename, version, debug_file, debug_id, code_id
            ));
        }
    }
    out
}

pub fn format_search(response: &SearchResponse) -> String {
    let mut output = String::new();

    output.push_str("# Search Results\n\n");
    output.push_str(&format!("Found **{}** crashes\n\n", response.total));

    if !response.hits.is_empty() {
        output.push_str("## Crashes\n\n");
        output.push_str(
            "| Crash ID | Product | Version | Platform | Channel | Build ID | Signature |\n",
        );
        output.push_str(
            "|----------|---------|---------|----------|---------|----------|----------|\n",
        );

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
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                hit.uuid, hit.product, hit.version, platform, channel, build, hit.signature
            ));
        }
        output.push('\n');
    }

    if !response.facets.is_empty() {
        output.push_str("## Aggregations\n\n");
        for (field, buckets) in &response.facets {
            output.push_str(&format!("### {}\n\n", field));
            for bucket in buckets {
                output.push_str(&format!(
                    "- **{}**: {} crashes\n",
                    bucket.term, bucket.count
                ));
            }
            output.push('\n');
        }
    }

    output
}

pub fn format_crash_pings(summary: &CrashPingsSummary) -> String {
    let mut output = String::new();

    output.push_str("# Crash Pings\n\n");
    if summary.date_from == summary.date_to {
        output.push_str(&format!("**Date:** {}\n\n", summary.date_from));
    } else {
        output.push_str(&format!(
            "**Date:** {} to {}\n\n",
            summary.date_from, summary.date_to
        ));
    }

    if let Some(ref sig) = summary.signature_filter {
        output.push_str(&format!(
            "**Signature:** `{}`\n\n**Matching pings:** {}\n\n",
            sig, summary.filtered_total
        ));
    } else {
        output.push_str(&format!("**Total pings:** {} (sampled)\n\n", summary.total));
    }

    if summary.items.is_empty() {
        output.push_str("No matching pings.\n");
    } else {
        let facet_label = &summary.facet_name;
        output.push_str(&format!("## By {}\n\n", facet_label));
        output.push_str(&format!("| {} | Count | % | Example IDs |\n", facet_label));
        output.push_str("|---|------:|--:|---|\n");
        for item in &summary.items {
            let ids = if item.example_ids.is_empty() {
                String::new()
            } else {
                item.example_ids
                    .iter()
                    .map(|id| format!("`{}`", id))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            output.push_str(&format!(
                "| {} | {} | {:.2}% | {} |\n",
                item.label, item.count, item.percentage, ids
            ));
        }
    }

    output
}

pub fn format_crash_ping_stack(summary: &CrashPingStackSummary) -> String {
    let mut output = String::new();

    output.push_str("# Crash Ping Stack\n\n");
    output.push_str(&format!("**Crash ID:** `{}`\n\n", summary.crash_id));
    output.push_str(&format!("**Date:** {}\n\n", summary.date));

    if summary.frames.is_empty() {
        if summary.java_exception.is_some() {
            output.push_str("## Java Exception\n\n");
            output.push_str("```json\n");
            if let Some(ref exc) = summary.java_exception {
                output.push_str(&serde_json::to_string_pretty(exc).unwrap_or_default());
                output.push('\n');
            }
            output.push_str("```\n");
        } else {
            output.push_str("No stack trace available.\n");
        }
    } else {
        output.push_str("## Stack Trace\n\n```\n");
        for (i, frame) in summary.frames.iter().enumerate() {
            output.push_str(&format!("#{} {}\n", i, format_frame_location(frame)));
        }
        output.push_str("```\n");
    }

    output
}

pub fn format_correlations(summary: &CorrelationsSummary) -> String {
    let mut output = String::new();

    output.push_str("# Correlations\n\n");
    output.push_str(&format!("**Signature:** `{}`\n\n", summary.signature));
    output.push_str(&format!(
        "- **Channel:** {}\n- **Data date:** {}\n- **Signature count:** {}\n- **Reference count:** {}\n\n",
        summary.channel, summary.date, summary.sig_count as u64, summary.ref_count
    ));

    if summary.items.is_empty() {
        output.push_str("No correlations found.\n");
    } else {
        output.push_str("| Sig % | Ref % | Attribute | Prior |\n");
        output.push_str("|------:|------:|-----------|-------|\n");

        for item in &summary.items {
            let prior_str = if let Some(prior) = &item.prior {
                format!(
                    "{:.2}% vs {:.2}% if {}",
                    prior.sig_pct, prior.ref_pct, prior.label
                )
            } else {
                String::new()
            };
            output.push_str(&format!(
                "| {:.2}% | {:.2}% | {} | {} |\n",
                item.sig_pct, item.ref_pct, item.label, prior_str
            ));
        }
    }

    output
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
                    cert_subject: Some("Mozilla Corporation".to_string()),
                },
                ModuleInfo {
                    filename: "ntdll.dll".to_string(),
                    debug_file: Some("ntdll.pdb".to_string()),
                    debug_id: Some("180BF1B9".to_string()),
                    code_id: Some("7ec9c15d".to_string()),
                    version: Some("6.2.19041.6456".to_string()),
                    cert_subject: Some("Microsoft Windows".to_string()),
                },
                ModuleInfo {
                    filename: "mozglue.dll".to_string(),
                    debug_file: Some("mozglue.pdb".to_string()),
                    debug_id: Some("AABBCCDD".to_string()),
                    code_id: Some("abc123".to_string()),
                    version: Some("148.0".to_string()),
                    cert_subject: Some("Mozilla Corporation".to_string()),
                },
            ],
        }
    }

    #[test]
    fn test_format_crash_markdown_header() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("# Crash Report"));
        assert!(output.contains("**Crash ID:** `247653e8-7a18-4836-97d1-42a720260120`"));
        assert!(
            output
                .contains("**Signature:** `mozilla::AudioDecoderInputTrack::EnsureTimeStretcher`")
        );
    }

    #[test]
    fn test_format_crash_markdown_details() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("## Details"));
        assert!(output.contains("- **Crash Reason:** SIGSEGV at `0x0` (null pointer)"));
        assert!(
            output
                .contains("- **Mozilla Crash Reason:** MOZ_RELEASE_ASSERT(mTimeStretcher->Init())")
        );
    }

    #[test]
    fn test_format_crash_markdown_product_info() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("- **Product:** Fenix 147.0.1"));
        assert!(output.contains("- **Platform:** Android 36 on SM-S918B (Android 36)"));
    }

    #[test]
    fn test_format_crash_markdown_stack_trace() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("## Stack Trace (GraphRunner)"));
        assert!(output.contains("```"));
        assert!(output.contains("#0 EnsureTimeStretcher @ AudioDecoderInputTrack.cpp:624"));
    }

    #[test]
    fn test_format_crash_markdown_all_threads() {
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

        assert!(output.contains("## All Threads"));
        assert!(output.contains("### Thread 0 (MainThread)"));
        assert!(output.contains("### Thread 1 (GraphRunner) **[CRASHING]**"));
    }

    #[test]
    fn test_format_crash_markdown_modules_none() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(!output.contains("## Modules"));
    }

    #[test]
    fn test_format_crash_markdown_modules_stack() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::Stack);

        assert!(output.contains("## Modules"));
        assert!(output.contains("| Module | Version | Debug File | Debug ID | Code ID |"));
        assert!(output.contains("| xul.dll | 148.0.0.3 | xul.pdb | F51BCD2A | 69934c4b |"));
        assert!(
            output.contains("| ntdll.dll | 6.2.19041.6456 | ntdll.pdb | 180BF1B9 | 7ec9c15d |")
        );
        // mozglue.dll not in stack frames
        assert!(!output.contains("mozglue.dll"));
    }

    #[test]
    fn test_format_crash_markdown_modules_full() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::Full);

        assert!(output.contains("## Modules"));
        assert!(output.contains("| xul.dll | 148.0.0.3 | xul.pdb | F51BCD2A | 69934c4b |"));
        assert!(
            output.contains("| ntdll.dll | 6.2.19041.6456 | ntdll.pdb | 180BF1B9 | 7ec9c15d |")
        );
        assert!(output.contains("| mozglue.dll | 148.0 | mozglue.pdb | AABBCCDD | abc123 |"));
    }

    fn sample_crash_summary_with_third_party_modules() -> CrashSummary {
        let mut summary = sample_crash_summary_with_modules();
        summary.modules.push(ModuleInfo {
            filename: "TmUmEvt64.dll".to_string(),
            debug_file: Some("TmUmEvt64.pdb".to_string()),
            debug_id: Some("F23993AD".to_string()),
            code_id: Some("696770e5".to_string()),
            version: Some("8.55.0.1429".to_string()),
            cert_subject: Some("Trend Micro, Inc.".to_string()),
        });
        summary.modules.push(ModuleInfo {
            filename: "unknown.dll".to_string(),
            debug_file: None,
            debug_id: None,
            code_id: None,
            version: None,
            cert_subject: None,
        });
        summary
    }

    #[test]
    fn test_format_crash_markdown_modules_third_party() {
        let summary = sample_crash_summary_with_third_party_modules();
        let output = format_crash(&summary, ModulesMode::ThirdParty);

        assert!(output.contains("## Modules"));
        assert!(output.contains("Signed By"));
        assert!(output.contains("| TmUmEvt64.dll |"));
        assert!(output.contains("| Trend Micro, Inc. |"));
        assert!(output.contains("| unknown.dll |"));
        assert!(output.contains("| unsigned |"));
        // Mozilla and Microsoft modules should NOT appear
        assert!(!output.contains("| xul.dll |"));
        assert!(!output.contains("| ntdll.dll |"));
        assert!(!output.contains("| mozglue.dll |"));
    }

    #[test]
    fn test_format_crash_markdown_modules_third_party_all_first_party() {
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::ThirdParty);
        assert!(!output.contains("## Modules"));
    }

    #[test]
    fn test_format_search_markdown_basic() {
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

        assert!(output.contains("# Search Results"));
        assert!(output.contains("Found **42** crashes"));
        assert!(output.contains("## Crashes"));
        assert!(output.contains(
            "| Crash ID | Product | Version | Platform | Channel | Build ID | Signature |"
        ));
    }

    #[test]
    fn test_format_search_markdown_with_facets() {
        let mut facets = HashMap::new();
        facets.insert(
            "version".to_string(),
            vec![FacetBucket {
                term: "120.0".to_string(),
                count: 50,
            }],
        );
        let response = SearchResponse {
            total: 50,
            hits: vec![],
            facets,
        };
        let output = format_search(&response);

        assert!(output.contains("## Aggregations"));
        assert!(output.contains("### version"));
        assert!(output.contains("- **120.0**: 50 crashes"));
    }

    use crate::models::bugs::{BugGroup, BugsSummary};
    use crate::models::{CorrelationItem, CorrelationItemPrior, CorrelationsSummary};

    #[test]
    fn test_format_bugs_markdown_with_results() {
        let summary = BugsSummary {
            bugs: vec![BugGroup {
                bug_id: 999999,
                signatures: vec!["OOM | small".to_string(), "OOM | large".to_string()],
            }],
        };
        let output = format_bugs(&summary);
        assert!(output.contains("# Bug Associations"));
        assert!(output.contains("| Bug | Signatures |"));
        assert!(output.contains("[999999](https://bugzilla.mozilla.org/show_bug.cgi?id=999999)"));
        assert!(output.contains("`OOM | small`"));
        assert!(output.contains("`OOM | large`"));
    }

    #[test]
    fn test_format_bugs_markdown_empty() {
        let summary = BugsSummary { bugs: vec![] };
        let output = format_bugs(&summary);
        assert!(output.contains("No bugs found."));
    }

    #[test]
    fn test_format_correlations_markdown_header() {
        let summary = CorrelationsSummary {
            signature: "TestSig".to_string(),
            channel: "release".to_string(),
            date: "2026-02-13".to_string(),
            sig_count: 220.0,
            ref_count: 79268,
            items: vec![CorrelationItem {
                label: "Module \"cscapi.dll\" = true".to_string(),
                sig_pct: 100.0,
                ref_pct: 24.51,
                prior: None,
            }],
        };
        let output = format_correlations(&summary);
        assert!(output.contains("# Correlations"));
        assert!(output.contains("**Signature:** `TestSig`"));
        assert!(output.contains("- **Channel:** release"));
        assert!(output.contains("| Sig % | Ref % | Attribute | Prior |"));
    }

    #[test]
    fn test_format_correlations_markdown_with_prior() {
        let summary = CorrelationsSummary {
            signature: "TestSig".to_string(),
            channel: "release".to_string(),
            date: "2026-02-13".to_string(),
            sig_count: 220.0,
            ref_count: 79268,
            items: vec![CorrelationItem {
                label: "startup_crash = null".to_string(),
                sig_pct: 29.55,
                ref_pct: 1.16,
                prior: Some(CorrelationItemPrior {
                    label: "process_type = parent".to_string(),
                    sig_pct: 50.91,
                    ref_pct: 4.58,
                }),
            }],
        };
        let output = format_correlations(&summary);
        assert!(output.contains("50.91% vs 4.58% if process_type = parent"));
    }

    #[test]
    fn test_format_correlations_markdown_empty() {
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
