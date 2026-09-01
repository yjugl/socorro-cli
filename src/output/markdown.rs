// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::commands::crash_pings::format_frame_location;
use crate::models::bugs::BugsSummary;
use crate::models::crash_pings::{CrashPingStackSummary, CrashPingsSummary};
use crate::models::{
    AsyncShutdownTimeout, CorrelationsSummary, CrashSummary, ModulesMode, SearchResponse,
    StackFrame,
};
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

/// The default crash report: identity, the `## Details` metadata list (which
/// includes the always-on crash-type annotations), the stack trace(s) and the
/// module table. The bulkier annotations live in [`format_annotations`], which
/// the crash command appends after this on request.
pub fn format_crash(summary: &CrashSummary, modules_mode: ModulesMode) -> String {
    let mut output = String::new();

    output.push_str("# Crash Report\n\n");
    output.push_str(&format!("**Crash ID:** `{}`\n\n", summary.crash_id));
    output.push_str(&format!("**Signature:** `{}`\n\n", summary.signature));

    output.push_str("## Details\n\n");

    // What kind of crash this is, from the always-on annotations. Each line is
    // independent, so a crash with none of them reads exactly as before.
    if let Some(report_type) = &summary.report_type {
        output.push_str(&format!("- **Report Type:** {}\n", report_type));
    }
    if let Some(process_type) = &summary.process_type {
        output.push_str(&format!("- **Process Type:** {}\n", process_type));
    }
    if let Some(uptime) = summary.uptime {
        output.push_str(&format!("- **Uptime:** {} seconds\n", uptime));
    }
    if let Some(thread_count) = summary.thread_count {
        output.push_str(&format!("- **Thread Count:** {}\n", thread_count));
    }
    // `false` is the answer for the overwhelming majority of crashes, so it is
    // noise: only a startup crash is worth a line.
    if summary.startup_crash == Some(true) {
        output.push_str("- **Startup Crash:** yes\n");
    }

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

/// The `--annotations` section: the crash annotations that are too bulky for
/// the default report but decide most shutdown-hang investigations. Appended
/// after [`format_crash`]'s output, so it opens at `##` level.
///
/// Short values become list items; long or multi-line ones get their own fenced
/// subsection, which also keeps a stray `|` or a leading `###!!!` out of the
/// markdown parser. Absent fields are omitted entirely, and a crash with no
/// annotations at all says so rather than showing an empty section.
pub fn format_annotations(summary: &CrashSummary) -> String {
    /// Longest value still rendered as a list item.
    const INLINE_MAX: usize = 200;

    fn add(
        label: &'static str,
        value: &Option<String>,
        items: &mut Vec<String>,
        blocks: &mut Vec<(&'static str, String)>,
    ) {
        let Some(raw) = value.as_deref() else {
            return;
        };
        // Annotations arrive with incidental whitespace: `app_notes` starts
        // with a literal newline, which would otherwise break the list.
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.contains('\n') || trimmed.chars().count() > INLINE_MAX {
            blocks.push((label, trimmed.to_string()));
        } else {
            items.push(format!("- **{}:** `{}`\n", label, trimmed));
        }
    }

    let mut items: Vec<String> = Vec::new();
    let mut blocks: Vec<(&'static str, String)> = Vec::new();

    add(
        "Shutdown Progress",
        &summary.shutdown_progress,
        &mut items,
        &mut blocks,
    );
    add(
        "Shutdown Reason",
        &summary.shutdown_reason,
        &mut items,
        &mut blocks,
    );
    add(
        "XPCOM Spin Event Loop Stack",
        &summary.xpcom_spin_event_loop_stack,
        &mut items,
        &mut blocks,
    );
    add("App Notes", &summary.app_notes, &mut items, &mut blocks);
    add(
        "Last Error Value",
        &summary.last_error_value,
        &mut items,
        &mut blocks,
    );
    if !summary.crash_inconsistencies.is_empty() {
        let list = summary
            .crash_inconsistencies
            .iter()
            .map(|c| format!("`{}`", c))
            .collect::<Vec<_>>()
            .join(", ");
        items.push(format!("- **Crash Inconsistencies:** {}\n", list));
    }
    add(
        "Topmost Filenames",
        &summary.topmost_filenames,
        &mut items,
        &mut blocks,
    );
    add(
        "Modules in Stack",
        &summary.modules_in_stack,
        &mut items,
        &mut blocks,
    );
    add(
        "Proto Signature",
        &summary.proto_signature,
        &mut items,
        &mut blocks,
    );

    let mut output = String::new();
    output.push_str("\n## Annotations\n\n");

    if items.is_empty() && blocks.is_empty() && summary.async_shutdown_timeout.is_none() {
        output.push_str("No annotations found.\n");
        return output;
    }

    for item in &items {
        output.push_str(item);
    }
    if !items.is_empty() {
        output.push('\n');
    }

    if let Some(timeout) = &summary.async_shutdown_timeout {
        output.push_str("### Async Shutdown Timeout\n\n");
        match timeout {
            AsyncShutdownTimeout::Parsed(data) => {
                let count = data.conditions.len();
                output.push_str(&format!("**Phase:** `{}`", data.phase));
                if count == 0 {
                    output.push_str(" (no pending conditions)\n\n");
                } else {
                    output.push_str(&format!(
                        " ({} pending condition{})\n\n",
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                    for (i, condition) in data.conditions.iter().enumerate() {
                        output.push_str(&format!("{}. **{}**\n", i + 1, condition.name));
                        match (&condition.filename, condition.line_number) {
                            (Some(file), Some(line)) => {
                                output.push_str(&format!("   - File: `{}:{}`\n", file, line))
                            }
                            (Some(file), None) => {
                                output.push_str(&format!("   - File: `{}`\n", file))
                            }
                            _ => {}
                        }
                        if let Some(state) = condition.state_display() {
                            output.push_str(&format!("   - State: `{}`\n", state));
                        }
                    }
                    output.push('\n');
                }
            }
            // Not JSON, or not the expected shape: keep it verbatim, fenced so
            // that whatever it contains cannot be misread as markdown.
            AsyncShutdownTimeout::Raw(raw) => {
                output.push_str(&format!("```\n{}\n```\n\n", raw.trim()));
            }
        }
    }

    for (label, value) in &blocks {
        output.push_str(&format!("### {}\n\n```\n{}\n```\n\n", label, value));
    }

    while output.ends_with("\n\n") {
        output.pop();
    }

    output
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
            ..Default::default()
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
            ..Default::default()
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

    /// The always-on annotation values of crash
    /// b98bbb81-3ff6-4825-991f-6a0b30260901: a parent-process shutdown hang
    /// that is not a startup crash.
    fn sample_crash_summary_with_annotations() -> CrashSummary {
        CrashSummary {
            report_type: Some("hang".to_string()),
            process_type: Some("parent".to_string()),
            uptime: Some(2175),
            startup_crash: Some(false),
            thread_count: Some(64),
            ..sample_crash_summary()
        }
    }

    #[test]
    fn test_format_crash_markdown_always_on_annotations() {
        let summary = sample_crash_summary_with_annotations();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("- **Report Type:** hang"), "{}", output);
        assert!(output.contains("- **Process Type:** parent"), "{}", output);
        assert!(output.contains("- **Uptime:** 2175 seconds"), "{}", output);
        assert!(output.contains("- **Thread Count:** 64"), "{}", output);
        // startup_crash is false here, so it must not add a line.
        assert!(!output.contains("Startup Crash"), "{}", output);
    }

    #[test]
    fn test_format_crash_markdown_no_always_on_annotations() {
        // sample_crash_summary() leaves all five at their default None.
        let output = format_crash(&sample_crash_summary(), ModulesMode::None);

        for label in [
            "Report Type",
            "Process Type",
            "Uptime",
            "Thread Count",
            "Startup Crash",
        ] {
            assert!(!output.contains(label), "unexpected {}: {}", label, output);
        }
    }

    #[test]
    fn test_format_crash_markdown_startup_crash_only_when_true() {
        let mut summary = sample_crash_summary_with_annotations();

        summary.startup_crash = Some(true);
        assert!(
            format_crash(&summary, ModulesMode::None).contains("- **Startup Crash:** yes"),
            "Some(true) must be shown"
        );

        summary.startup_crash = Some(false);
        assert!(
            !format_crash(&summary, ModulesMode::None).contains("Startup Crash"),
            "Some(false) must be omitted"
        );

        summary.startup_crash = None;
        assert!(
            !format_crash(&summary, ModulesMode::None).contains("Startup Crash"),
            "None must be omitted"
        );
    }

    /// The opt-in annotation values of crash
    /// b98bbb81-3ff6-4825-991f-6a0b30260901, including its leading-newline
    /// `app_notes` and its ` | `-separated `proto_signature`.
    fn sample_annotations_summary() -> CrashSummary {
        CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::parse(
                r#"{"phase":"profile-before-change","conditions":[
                    {"name":"ServiceWorkerRegistrar: Flushing data",
                     "state":{"saveDataRunnableDispatched":false,"shuttingDown":false},
                     "filename":"..\\..\\checkouts\\gecko\\dom\\serviceworkers\\ServiceWorkerRegistrar.cpp",
                     "lineNumber":1566},
                    {"name":"ASRouterStorage: flush pending writes",
                     "state":{"pending":1},
                     "filename":"resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs",
                     "lineNumber":50},
                    {"name":"ShieldRecipeClient: Cleaning up",
                     "state":"(none)",
                     "filename":"resource://normandy/lib/CleanupManager.sys.mjs",
                     "lineNumber":39}]}"#,
            )),
            shutdown_progress: Some("profile-before-change".to_string()),
            shutdown_reason: Some("AppClose".to_string()),
            xpcom_spin_event_loop_stack: Some(
                "default: AsyncShutdown Spinner for profile-before-change".to_string(),
            ),
            app_notes: Some(
                "\n-L1000-W0000100-T1) DWrite? DWrite+ WR! WR+ xpcom_runtime_abort(###!!! ABORT: file checkouts\\gecko\\dom\\serviceworkers\\ServiceWorkerRegistrar.cpp:1566)"
                    .to_string(),
            ),
            last_error_value: Some("ERROR_SUCCESS".to_string()),
            crash_inconsistencies: vec!["crashing thread frames mismatch".to_string()],
            topmost_filenames: Some("mfbt/Assertions.h".to_string()),
            modules_in_stack: Some(
                "firefox.exe/77DFC624CE9E472E4C4C44205044422E1;xul.dll/87B0A0D5FAAC4E194C4C44205044422E1"
                    .to_string(),
            ),
            proto_signature: Some(
                "MOZ_Crash | Abort | NS_PrintStackTrace | NS_DebugBreak | nsDebugImpl::Abort"
                    .to_string(),
            ),
            ..sample_crash_summary()
        }
    }

    #[test]
    fn test_format_annotations_markdown_all_fields() {
        let output = format_annotations(&sample_annotations_summary());

        assert!(output.starts_with("\n## Annotations\n\n"), "{}", output);
        assert!(output.contains("### Async Shutdown Timeout"), "{}", output);
        assert!(
            output.contains("- **Shutdown Progress:** `profile-before-change`"),
            "{}",
            output
        );
        assert!(
            output.contains("- **Shutdown Reason:** `AppClose`"),
            "{}",
            output
        );
        assert!(
            output.contains(
                "- **XPCOM Spin Event Loop Stack:** `default: AsyncShutdown Spinner for profile-before-change`"
            ),
            "{}",
            output
        );
        assert!(output.contains("- **App Notes:** `-L1000"), "{}", output);
        assert!(
            output.contains("- **Last Error Value:** `ERROR_SUCCESS`"),
            "{}",
            output
        );
        assert!(
            output.contains("- **Crash Inconsistencies:** `crashing thread frames mismatch`"),
            "{}",
            output
        );
        assert!(
            output.contains("- **Topmost Filenames:** `mfbt/Assertions.h`"),
            "{}",
            output
        );
        assert!(
            output.contains("- **Modules in Stack:** `firefox.exe/"),
            "{}",
            output
        );
        assert!(output.contains("MOZ_Crash | Abort"), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_absent_fields_omitted() {
        let summary = CrashSummary {
            shutdown_reason: Some("AppClose".to_string()),
            ..sample_crash_summary()
        };
        let output = format_annotations(&summary);

        assert!(
            output.contains("- **Shutdown Reason:** `AppClose`"),
            "{}",
            output
        );
        assert!(!output.contains("Shutdown Progress"), "{}", output);
        assert!(!output.contains("App Notes"), "{}", output);
        assert!(!output.contains("Async Shutdown Timeout"), "{}", output);
        assert!(!output.contains("No annotations found."), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_empty() {
        // Nothing present, including an empty crash_inconsistencies Vec: the
        // reader must be able to tell the flag worked.
        let output = format_annotations(&sample_crash_summary());

        assert!(output.contains("## Annotations"), "{}", output);
        assert!(output.contains("No annotations found."), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_shutdown_conditions() {
        let output = format_annotations(&sample_annotations_summary());

        assert!(
            output.contains("**Phase:** `profile-before-change` (3 pending conditions)"),
            "{}",
            output
        );
        assert!(
            output.contains("1. **ServiceWorkerRegistrar: Flushing data**"),
            "{}",
            output
        );
        assert!(
            output.contains("ServiceWorkerRegistrar.cpp:1566`"),
            "{}",
            output
        );
        assert!(
            output.contains(
                "   - State: `{\"saveDataRunnableDispatched\":false,\"shuttingDown\":false}`"
            ),
            "{}",
            output
        );
        assert!(
            output.contains("3. **ShieldRecipeClient: Cleaning up**"),
            "{}",
            output
        );
        // A bare-string state renders as-is.
        assert!(output.contains("   - State: `(none)`"), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_condition_without_file() {
        let summary = CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::parse(
                r#"{"phase":"quit-application","conditions":[{"name":"Blocker"}]}"#,
            )),
            ..sample_crash_summary()
        };
        let output = format_annotations(&summary);

        assert!(
            output.contains("**Phase:** `quit-application` (1 pending condition)"),
            "{}",
            output
        );
        assert!(output.contains("1. **Blocker**"), "{}", output);
        assert!(!output.contains("- File:"), "{}", output);
        assert!(!output.contains("- State:"), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_raw_timeout() {
        let summary = CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::Raw(
                "  not json at all {  ".to_string(),
            )),
            ..sample_crash_summary()
        };
        let output = format_annotations(&summary);

        assert!(output.contains("### Async Shutdown Timeout"), "{}", output);
        // Verbatim, trimmed, and fenced so it cannot be misread as markdown.
        assert!(output.contains("```\nnot json at all {\n```"), "{}", output);
        assert!(!output.contains("No annotations found."), "{}", output);
    }

    #[test]
    fn test_format_annotations_markdown_app_notes_leading_newline_trimmed() {
        let summary = CrashSummary {
            app_notes: Some(
                "\n-L1000-W0000100-T1) DWrite? xpcom_runtime_abort(###!!! ABORT)".to_string(),
            ),
            ..sample_crash_summary()
        };
        let output = format_annotations(&summary);

        // No stray blank line, and no line may begin with the ###!!! text,
        // which markdown would otherwise render as a heading.
        assert!(
            output.contains("- **App Notes:** `-L1000-W0000100-T1)"),
            "{}",
            output
        );
        assert!(!output.contains("`\n-L1000"), "{}", output);
        assert!(
            !output.lines().any(|l| l.starts_with("###!!!")),
            "{}",
            output
        );
    }

    #[test]
    fn test_format_annotations_markdown_long_proto_signature_is_fenced() {
        // ~1,000 chars in the live data, and full of ` | ` separators that
        // would break a markdown table.
        let long_signature = vec!["MOZ_Crash"; 40].join(" | ");
        let summary = CrashSummary {
            proto_signature: Some(long_signature.clone()),
            ..sample_crash_summary()
        };
        let output = format_annotations(&summary);

        assert!(output.contains("### Proto Signature"), "{}", output);
        assert!(
            output.contains(&format!("```\n{}\n```", long_signature)),
            "{}",
            output
        );
        // Not a list item, and not a table row.
        assert!(!output.contains("- **Proto Signature:**"), "{}", output);
        assert!(!output.lines().any(|l| l.starts_with('|')), "{}", output);
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
