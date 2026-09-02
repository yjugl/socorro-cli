// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Token-optimized plain-text formatters (the default output format).
//!
//! For a crash this emits the identity and metadata lines (`sig:`, `reason:`,
//! `type:`, `product:`, ...), the crashing thread's stack (or every thread),
//! and optionally the module list. Under `--all-threads` the stacks are
//! preceded by a `threads: N total, M distinct stacks shown` line (with
//! `stack` singular when `M` is 1), and threads
//! that [`CrashSummary`] grouped as having identical truncated stacks share one
//! `stack[K threads: ...]:` block naming every member. The crash annotations
//! gathered in [`CrashSummary`] are split in two: the cheap ones share the
//! always-on `type:` line, while the verbose ones are rendered only by
//! [`format_annotations`], which the caller appends on request.

use crate::commands::crash_pings::format_frame_location;
use crate::models::bugs::BugsSummary;
use crate::models::crash_pings::{CrashPingStackSummary, CrashPingsSummary};
use crate::models::{
    AsyncShutdownTimeout, CorrelationsSummary, CrashSummary, ModulesMode, SearchResponse,
    ShutdownCondition, StackFrame, ThreadSummary,
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

/// `index:name` for one thread, with `unknown` standing in for a missing name.
fn thread_label(index: usize, name: Option<&str>) -> String {
    format!("{}:{}", index, name.unwrap_or("unknown"))
}

/// `"s"` unless `count` is exactly 1, for pluralizing a noun inline.
///
/// Takes `u64` because one caller counts threads from a `u64` annotation while
/// the others count collection lengths; widening a `usize` is lossless on every
/// target Rust supports, whereas narrowing the annotation would not be.
fn plural_s(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// The `threads:` line introducing the `--all-threads` section.
///
/// Both counts are derived from the summaries rather than stored: `distinct` is
/// how many `stack[...]` blocks follow, and `total` counts each block's
/// representative plus the threads folded into it. It is emitted whenever any
/// thread summary exists — even when nothing was folded and the two numbers are
/// equal — so machine consumers can rely on the line being there.
///
/// `stack` agrees in number with `distinct`, which is routinely 1 at low
/// `--depth` where every thread collapses into one group. `threads:` is a fixed
/// label rather than a counted noun, so it stays plural throughout.
fn format_thread_counts(threads: &[ThreadSummary]) -> String {
    let distinct = threads.len();
    let total: usize = threads.iter().map(|t| 1 + t.identical_threads.len()).sum();
    format!(
        "threads: {} total, {} distinct stack{} shown\n",
        total,
        distinct,
        plural_s(distinct as u64)
    )
}

/// One `stack[...]:` header line.
///
/// A summary with nothing folded into it keeps the historical single-thread
/// form, `stack[thread 0:GeckoMain [CRASHING]]:`. One that represents a group
/// names every member as `index:name`, representative first and then the
/// `identical_threads` in increasing index order:
/// `stack[8 threads: 5:TaskController #0, 6:TaskController #1, ...]:`.
///
/// The line is never wrapped and no member is elided, however long it gets: a
/// two-space-indented continuation would be indistinguishable from a frame
/// line, and compact output is parsed by tooling.
fn format_thread_header(thread: &ThreadSummary) -> String {
    let crash_marker = if thread.is_crashing {
        " [CRASHING]"
    } else {
        ""
    };
    let representative = format!(
        "{}{}",
        thread_label(thread.thread_index, thread.thread_name.as_deref()),
        crash_marker
    );

    if thread.identical_threads.is_empty() {
        return format!("stack[thread {}]:\n", representative);
    }

    let mut members = Vec::with_capacity(1 + thread.identical_threads.len());
    members.push(representative);
    for member in &thread.identical_threads {
        members.push(thread_label(
            member.thread_index,
            member.thread_name.as_deref(),
        ));
    }

    format!(
        "stack[{} threads: {}]:\n",
        members.len(),
        members.join(", ")
    )
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

    output.push_str(&format_type_line(summary));

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
        output.push_str(&format_thread_counts(&summary.all_threads));
        output.push('\n');
        for thread in &summary.all_threads {
            output.push_str(&format_thread_header(thread));

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
    out.push_str("\nmodules:\n");
    for m in &modules {
        let version = m.version.as_deref().unwrap_or("?");
        let debug_file = m.debug_file.as_deref().unwrap_or("?");
        let debug_id = m.debug_id.as_deref().unwrap_or("?");
        let code_id = m.code_id.as_deref().unwrap_or("?");
        if show_cert {
            let cert = m.cert_subject.as_deref().unwrap_or("unsigned");
            out.push_str(&format!(
                "  {} {} | {} | {} | {} | {}\n",
                m.filename, version, debug_file, debug_id, code_id, cert
            ));
        } else {
            out.push_str(&format!(
                "  {} {} | {} | {} | {}\n",
                m.filename, version, debug_file, debug_id, code_id
            ));
        }
    }
    out
}

/// Collapse an annotation onto a single line. Annotations arrive with
/// surrounding whitespace — `app_notes` always begins with a newline — and can
/// carry interior newlines, either of which would break the compact format's
/// one-field-per-line contract.
fn one_line(value: &str) -> String {
    value
        .split('\n')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The always-on `type:` line: report type, process, uptime, thread count and,
/// when true, that this was a startup crash. Returns the empty string when the
/// crash carries none of those, so no blank line is ever emitted.
///
/// `thread` agrees in number with the count, so a single-threaded crash reads
/// `| 1 thread`. Single-threaded crashes are not exotic: SuperSearch reports
/// 1102 of them for `thread_count=1` as of 2026-09-02.
fn format_type_line(summary: &CrashSummary) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(report_type) = &summary.report_type {
        parts.push(report_type.clone());
    }
    if let Some(process_type) = &summary.process_type {
        parts.push(process_type.clone());
    }
    if let Some(uptime) = summary.uptime {
        parts.push(format!("uptime {}s", uptime));
    }
    if let Some(thread_count) = summary.thread_count {
        parts.push(format!("{} thread{}", thread_count, plural_s(thread_count)));
    }
    // Only the true case carries information: the overwhelming majority of
    // crashes are not startup crashes, so `startup_crash: false` is noise.
    if summary.startup_crash == Some(true) {
        parts.push("startup".to_string());
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("type: {}\n", parts.join(" | "))
    }
}

/// One line for a shutdown condition's `state`. A JSON object becomes
/// space-separated `key=value` pairs, which is shorter and easier to read than
/// the raw JSON; anything else falls back to
/// [`ShutdownCondition::state_display`].
fn format_condition_state(condition: &ShutdownCondition) -> Option<String> {
    let rendered = if let serde_json::Value::Object(map) = &condition.state {
        map.iter()
            .map(|(key, value)| match value {
                serde_json::Value::String(s) => format!("{}={}", key, one_line(s)),
                other => format!("{}={}", key, other),
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        one_line(&condition.state_display()?)
    };

    Some(rendered).filter(|s| !s.is_empty())
}

/// The `shutdown:` sub-section: the phase that timed out, how many blockers
/// were still pending, and one block per blocker. An unparseable annotation is
/// emitted verbatim on the `shutdown:` line rather than dropped.
fn format_shutdown_timeout(shutdown: &AsyncShutdownTimeout) -> String {
    let data = match shutdown {
        AsyncShutdownTimeout::Raw(raw) => {
            let raw = one_line(raw);
            return if raw.is_empty() {
                String::new()
            } else {
                format!("  shutdown: {}\n", raw)
            };
        }
        AsyncShutdownTimeout::Parsed(data) => data,
    };

    let noun = if data.conditions.len() == 1 {
        "condition"
    } else {
        "conditions"
    };
    let mut out = format!(
        "  shutdown: phase {}, {} {}\n",
        data.phase,
        data.conditions.len(),
        noun
    );

    for condition in &data.conditions {
        out.push_str(&format!("    - {}\n", one_line(&condition.name)));
        if let Some(filename) = &condition.filename {
            match condition.line_number {
                Some(line) => out.push_str(&format!("      {}:{}\n", filename, line)),
                None => out.push_str(&format!("      {}\n", filename)),
            }
        }
        if let Some(state) = format_condition_state(condition) {
            out.push_str(&format!("      {}\n", state));
        }
    }

    out
}

/// The opt-in `annotations:` section, appended after the crash body when the
/// caller asks for it. Absent annotations are omitted; when none of them is
/// present the section still says so, so an agent can tell the difference
/// between "nothing to report" and "the flag did nothing".
pub fn format_annotations(summary: &CrashSummary) -> String {
    let mut body = String::new();

    if let Some(shutdown) = &summary.async_shutdown_timeout {
        body.push_str(&format_shutdown_timeout(shutdown));
    }

    let inconsistencies = if summary.crash_inconsistencies.is_empty() {
        None
    } else {
        Some(summary.crash_inconsistencies.join(", "))
    };

    let fields: Vec<(&str, Option<String>)> = vec![
        (
            "shutdown_progress",
            summary.shutdown_progress.as_deref().map(one_line),
        ),
        (
            "shutdown_reason",
            summary.shutdown_reason.as_deref().map(one_line),
        ),
        (
            "spin_event_loop",
            summary.xpcom_spin_event_loop_stack.as_deref().map(one_line),
        ),
        ("app_notes", summary.app_notes.as_deref().map(one_line)),
        (
            "last_error",
            summary.last_error_value.as_deref().map(one_line),
        ),
        ("crash_inconsistencies", inconsistencies),
        (
            "topmost_filenames",
            summary.topmost_filenames.as_deref().map(one_line),
        ),
        (
            "modules_in_stack",
            summary.modules_in_stack.as_deref().map(one_line),
        ),
        (
            "proto_signature",
            summary.proto_signature.as_deref().map(one_line),
        ),
    ];

    for (key, value) in fields {
        match value {
            Some(value) if !value.is_empty() => {
                body.push_str(&format!("  {}: {}\n", key, value));
            }
            _ => {}
        }
    }

    if body.is_empty() {
        return "annotations: (none)\n".to_string();
    }

    format!("annotations:\n{}", body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AsyncShutdownTimeoutData, CrashHit, CrashSummary, FacetBucket, ModuleInfo, ModulesMode,
        ThreadRef, ThreadSummary,
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
                ..Default::default()
            },
            ThreadSummary {
                thread_index: 1,
                thread_name: Some("GraphRunner".to_string()),
                frames: vec![],
                is_crashing: true,
                ..Default::default()
            },
        ];
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("stack[thread 0:MainThread]:"));
        assert!(output.contains("stack[thread 1:GraphRunner [CRASHING]]:"));
    }

    /// A crashing thread on its own plus one group of four idle threads, the
    /// last of which has no name. Total is 5, distinct is 2.
    fn sample_crash_summary_with_grouped_threads() -> CrashSummary {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![
            ThreadSummary {
                thread_index: 0,
                thread_name: Some("MainThread".to_string()),
                frames: vec![],
                is_crashing: false,
                identical_threads: vec![],
            },
            ThreadSummary {
                thread_index: 5,
                thread_name: Some("TaskController #0".to_string()),
                frames: vec![],
                is_crashing: false,
                identical_threads: vec![
                    ThreadRef {
                        thread_index: 6,
                        thread_name: Some("TaskController #1".to_string()),
                    },
                    ThreadRef {
                        thread_index: 7,
                        thread_name: Some("TaskController #2".to_string()),
                    },
                    ThreadRef {
                        thread_index: 9,
                        thread_name: None,
                    },
                ],
            },
        ];
        summary
    }

    /// Pull the single line containing `needle` out of `output`, panicking if
    /// it is absent or ambiguous.
    fn line_containing<'a>(output: &'a str, needle: &str) -> &'a str {
        let matches: Vec<&str> = output.lines().filter(|l| l.contains(needle)).collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one line containing {:?}, got {:?}",
            needle,
            matches
        );
        matches[0]
    }

    #[test]
    fn test_format_crash_all_threads_count_line() {
        let summary = sample_crash_summary_with_grouped_threads();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("threads: 5 total, 2 distinct stacks shown\n"));
        // The count line is followed by a blank line, then the first stack.
        assert!(output.contains("threads: 5 total, 2 distinct stacks shown\n\nstack["));
    }

    #[test]
    fn test_format_crash_all_threads_count_line_present_when_nothing_grouped() {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![
            ThreadSummary {
                thread_index: 0,
                thread_name: Some("MainThread".to_string()),
                frames: vec![],
                is_crashing: false,
                identical_threads: vec![],
            },
            ThreadSummary {
                thread_index: 1,
                thread_name: Some("GraphRunner".to_string()),
                frames: vec![],
                is_crashing: true,
                identical_threads: vec![],
            },
        ];
        let output = format_crash(&summary, ModulesMode::None);

        assert!(output.contains("threads: 2 total, 2 distinct stacks shown\n"));
    }

    /// One thread, on its own: both counts are 1, so `stack` is singular.
    #[test]
    fn test_format_crash_all_threads_count_line_singular_stack() {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![ThreadSummary {
            thread_index: 0,
            thread_name: Some("MainThread".to_string()),
            frames: vec![],
            is_crashing: true,
            identical_threads: vec![],
        }];
        let output = format_crash(&summary, ModulesMode::None);

        assert_eq!(
            line_containing(&output, "distinct stack"),
            "threads: 1 total, 1 distinct stack shown"
        );
    }

    /// Two threads folded into one group: `threads:` is a fixed label and stays
    /// as it is, while `stack` follows the distinct count and goes singular.
    #[test]
    fn test_format_crash_all_threads_count_line_plural_total_singular_stack() {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![ThreadSummary {
            thread_index: 0,
            thread_name: Some("MainThread".to_string()),
            frames: vec![],
            is_crashing: false,
            identical_threads: vec![ThreadRef {
                thread_index: 1,
                thread_name: Some("GraphRunner".to_string()),
            }],
        }];
        let output = format_crash(&summary, ModulesMode::None);

        assert_eq!(
            line_containing(&output, "distinct stack"),
            "threads: 2 total, 1 distinct stack shown"
        );
    }

    #[test]
    fn test_format_crash_all_threads_single_member_header_unchanged() {
        let summary = sample_crash_summary_with_grouped_threads();
        let output = format_crash(&summary, ModulesMode::None);

        // Byte-exact historical form for an ungrouped thread.
        assert_eq!(
            line_containing(&output, "MainThread"),
            "stack[thread 0:MainThread]:"
        );
    }

    #[test]
    fn test_format_crash_all_threads_group_header_lists_every_member() {
        let summary = sample_crash_summary_with_grouped_threads();
        let output = format_crash(&summary, ModulesMode::None);

        assert_eq!(
            line_containing(&output, "TaskController #0"),
            "stack[4 threads: 5:TaskController #0, 6:TaskController #1, \
             7:TaskController #2, 9:unknown]:"
        );
    }

    #[test]
    fn test_format_crash_all_threads_group_header_is_one_line() {
        let summary = sample_crash_summary_with_grouped_threads();
        let output = format_crash(&summary, ModulesMode::None);

        let start = output
            .find("stack[4 threads:")
            .expect("group header missing");
        let end = output[start..]
            .find("]:")
            .expect("group header unterminated")
            + start;
        assert!(
            !output[start..end].contains('\n'),
            "group header was wrapped: {:?}",
            &output[start..end]
        );
    }

    #[test]
    fn test_format_crash_all_threads_unnamed_member_renders_unknown() {
        let summary = sample_crash_summary_with_grouped_threads();
        let output = format_crash(&summary, ModulesMode::None);

        assert!(line_containing(&output, "TaskController #0").contains("9:unknown"));
    }

    #[test]
    fn test_format_crash_all_threads_group_marks_crashing_representative() {
        let mut summary = sample_crash_summary();
        summary.all_threads = vec![ThreadSummary {
            thread_index: 3,
            thread_name: Some("Worker".to_string()),
            frames: vec![],
            is_crashing: true,
            identical_threads: vec![ThreadRef {
                thread_index: 4,
                thread_name: Some("Worker 2".to_string()),
            }],
        }];
        let output = format_crash(&summary, ModulesMode::None);

        // The model never groups the crashing thread, but if it ever did the
        // marker must sit on the representative rather than on the last member.
        assert_eq!(
            line_containing(&output, "Worker"),
            "stack[2 threads: 3:Worker [CRASHING], 4:Worker 2]:"
        );
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
                ..Default::default()
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
                ..Default::default()
            },
        ];
        let output = format_crash(&summary, ModulesMode::Stack);

        // Both mozglue.dll and xul.dll are in threads, so both should appear
        assert!(output.contains("mozglue.dll"));
        assert!(output.contains("xul.dll"));
        // ntdll.dll is NOT in any thread frame
        assert!(!output.contains("ntdll.dll"));
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
    fn test_format_crash_modules_third_party() {
        let summary = sample_crash_summary_with_third_party_modules();
        let output = format_crash(&summary, ModulesMode::ThirdParty);

        assert!(output.contains("modules:"));
        // Third-party signed module should appear with cert info
        assert!(output.contains("TmUmEvt64.dll 8.55.0.1429"));
        assert!(output.contains("Trend Micro, Inc."));
        // Unsigned module should appear
        assert!(output.contains("unknown.dll"));
        assert!(output.contains("unsigned"));
        // Mozilla and Microsoft modules should NOT appear
        assert!(!output.contains("xul.dll"));
        assert!(!output.contains("ntdll.dll"));
        assert!(!output.contains("mozglue.dll"));
    }

    #[test]
    fn test_format_crash_modules_third_party_all_first_party() {
        // When all modules are Mozilla/Microsoft, third-party shows nothing
        let summary = sample_crash_summary_with_modules();
        let output = format_crash(&summary, ModulesMode::ThirdParty);
        assert!(!output.contains("modules:"));
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

    /// Shaped after crash b98bbb81-3ff6-4825-991f-6a0b30260901: a
    /// parent-process shutdown hang with 64 threads that was *not* a startup
    /// crash.
    fn hang_summary() -> CrashSummary {
        CrashSummary {
            crash_id: "b98bbb81-3ff6-4825-991f-6a0b30260901".to_string(),
            signature: "AsyncShutdownTimeout | profile-before-change".to_string(),
            reason: Some("EXCEPTION_BREAKPOINT".to_string()),
            address: Some("0x00007fffba3d2c6e".to_string()),
            product: "Firefox".to_string(),
            version: "157.0a1".to_string(),
            platform: "Windows NT 10.0.26200".to_string(),
            report_type: Some("hang".to_string()),
            process_type: Some("parent".to_string()),
            uptime: Some(2175),
            startup_crash: Some(false),
            thread_count: Some(64),
            ..Default::default()
        }
    }

    fn type_line(output: &str) -> Option<&str> {
        output.lines().find(|line| line.starts_with("type: "))
    }

    #[test]
    fn test_compact_crash_type_line_exact() {
        let output = format_crash(&hang_summary(), ModulesMode::Stack);
        assert_eq!(
            type_line(&output),
            Some("type: hang | parent | uptime 2175s | 64 threads")
        );
    }

    #[test]
    fn test_compact_crash_type_line_follows_reason() {
        let output = format_crash(&hang_summary(), ModulesMode::Stack);
        let lines: Vec<&str> = output.lines().collect();
        let reason_idx = lines
            .iter()
            .position(|line| line.starts_with("reason: "))
            .expect("reason line");
        assert!(
            lines[reason_idx + 1].starts_with("type: "),
            "type line must follow reason, got {:?}",
            lines[reason_idx + 1]
        );
    }

    #[test]
    fn test_compact_crash_type_line_follows_sig_without_reason() {
        let mut summary = hang_summary();
        summary.reason = None;
        summary.address = None;
        let output = format_crash(&summary, ModulesMode::Stack);
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines[1].starts_with("sig: "), "got {:?}", lines[1]);
        assert!(lines[2].starts_with("type: "), "got {:?}", lines[2]);
    }

    #[test]
    fn test_compact_crash_type_line_absent_when_no_components() {
        // The stock fixture has none of the five annotations, so no empty
        // `type:` line may appear.
        let output = format_crash(&sample_crash_summary(), ModulesMode::None);
        assert_eq!(type_line(&output), None, "output was:\n{}", output);
    }

    #[test]
    fn test_compact_crash_type_line_startup_crash() {
        let mut summary = hang_summary();

        summary.startup_crash = Some(true);
        assert_eq!(
            type_line(&format_crash(&summary, ModulesMode::None)),
            Some("type: hang | parent | uptime 2175s | 64 threads | startup")
        );

        summary.startup_crash = Some(false);
        assert_eq!(
            type_line(&format_crash(&summary, ModulesMode::None)),
            Some("type: hang | parent | uptime 2175s | 64 threads")
        );

        summary.startup_crash = None;
        assert_eq!(
            type_line(&format_crash(&summary, ModulesMode::None)),
            Some("type: hang | parent | uptime 2175s | 64 threads")
        );
    }

    #[test]
    fn test_compact_crash_type_line_single_component() {
        let summary = CrashSummary {
            process_type: Some("content".to_string()),
            ..Default::default()
        };
        assert_eq!(
            type_line(&format_crash(&summary, ModulesMode::None)),
            Some("type: content")
        );
    }

    /// A single-threaded crash: `thread` is singular. Real and not rare --
    /// `7b7dcaf0-2985-43f6-af63-6209d0260826` is one.
    #[test]
    fn test_compact_crash_type_line_thread_count_singular() {
        let mut summary = hang_summary();
        summary.thread_count = Some(1);
        assert_eq!(
            type_line(&format_crash(&summary, ModulesMode::None)),
            Some("type: hang | parent | uptime 2175s | 1 thread")
        );
    }

    /// Every count other than 1 stays plural, including 0 and the two-thread
    /// case that sits right next to the singular boundary.
    #[test]
    fn test_compact_crash_type_line_thread_count_plural() {
        let mut summary = hang_summary();

        for count in [0u64, 2, 64] {
            summary.thread_count = Some(count);
            assert_eq!(
                type_line(&format_crash(&summary, ModulesMode::None)),
                Some(format!("type: hang | parent | uptime 2175s | {} threads", count).as_str())
            );
        }
    }

    fn condition(
        name: &str,
        filename: Option<&str>,
        line: Option<u64>,
        state: serde_json::Value,
    ) -> ShutdownCondition {
        ShutdownCondition {
            name: name.to_string(),
            filename: filename.map(str::to_string),
            line_number: line,
            state,
        }
    }

    /// Shaped after the same crash's `async_shutdown_timeout` blob.
    fn annotated_summary() -> CrashSummary {
        CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::Parsed(AsyncShutdownTimeoutData {
                phase: "profile-before-change".to_string(),
                conditions: vec![
                    condition(
                        "ServiceWorkerRegistrar: Flushing data",
                        Some(
                            "..\\..\\checkouts\\gecko\\dom\\serviceworkers\\ServiceWorkerRegistrar.cpp",
                        ),
                        Some(1566),
                        serde_json::json!({"saveDataRunnableDispatched": false, "shuttingDown": false}),
                    ),
                    condition(
                        "ASRouterStorage: flush pending writes",
                        Some("resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs"),
                        Some(50),
                        serde_json::json!({"pending": 1}),
                    ),
                    condition(
                        "ShieldRecipeClient: Cleaning up",
                        Some("resource://normandy/lib/CleanupManager.sys.mjs"),
                        Some(39),
                        serde_json::json!("(none)"),
                    ),
                ],
            })),
            shutdown_progress: Some("profile-before-change".to_string()),
            shutdown_reason: Some("AppClose".to_string()),
            xpcom_spin_event_loop_stack: Some(
                "default: AsyncShutdown Spinner for profile-before-change".to_string(),
            ),
            app_notes: Some("\n-L1000-W0000100-T1) DWrite? DWrite+ WR! WR+".to_string()),
            last_error_value: Some("ERROR_SUCCESS".to_string()),
            topmost_filenames: Some("mfbt/Assertions.h".to_string()),
            modules_in_stack: Some("firefox.exe/77DFC624;xul.dll/87B0A0D5".to_string()),
            proto_signature: Some("MOZ_Crash | Abort | NS_DebugBreak".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_compact_annotations_field_order() {
        let output = format_annotations(&annotated_summary());
        let keys: Vec<&str> = output
            .lines()
            .filter(|line| line.starts_with("  ") && !line.starts_with("    "))
            .map(|line| line.trim_start().split(':').next().unwrap())
            .collect();
        assert_eq!(
            keys,
            vec![
                "shutdown",
                "shutdown_progress",
                "shutdown_reason",
                "spin_event_loop",
                "app_notes",
                "last_error",
                "topmost_filenames",
                "modules_in_stack",
                "proto_signature",
            ]
        );
        assert!(output.starts_with("annotations:\n"), "got {:?}", output);
    }

    #[test]
    fn test_compact_annotations_none() {
        assert_eq!(
            format_annotations(&CrashSummary::default()),
            "annotations: (none)\n"
        );
    }

    #[test]
    fn test_compact_annotations_shutdown_conditions() {
        let output = format_annotations(&annotated_summary());
        assert!(
            output.contains("  shutdown: phase profile-before-change, 3 conditions\n"),
            "got:\n{}",
            output
        );
        assert!(
            output.contains("    - ASRouterStorage: flush pending writes\n      resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs:50\n      pending=1\n"),
            "got:\n{}",
            output
        );
        // An object state renders as key=value pairs, not raw JSON.
        assert!(
            output.contains("      saveDataRunnableDispatched=false shuttingDown=false\n"),
            "got:\n{}",
            output
        );
        // A bare-string state falls back to state_display().
        assert!(output.contains("      (none)\n"), "got:\n{}", output);
    }

    #[test]
    fn test_compact_annotations_shutdown_condition_without_location() {
        let summary = CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::Parsed(AsyncShutdownTimeoutData {
                phase: "quit-application".to_string(),
                conditions: vec![
                    condition("No location", None, None, serde_json::Value::Null),
                    condition("File only", Some("Foo.cpp"), None, serde_json::Value::Null),
                ],
            })),
            ..Default::default()
        };
        assert_eq!(
            format_annotations(&summary),
            "annotations:\n  shutdown: phase quit-application, 2 conditions\n    - No location\n    - File only\n      Foo.cpp\n"
        );
    }

    #[test]
    fn test_compact_annotations_shutdown_raw_verbatim() {
        let summary = CrashSummary {
            async_shutdown_timeout: Some(AsyncShutdownTimeout::Raw(
                "not json at all {".to_string(),
            )),
            ..Default::default()
        };
        assert_eq!(
            format_annotations(&summary),
            "annotations:\n  shutdown: not json at all {\n"
        );
    }

    #[test]
    fn test_compact_annotations_app_notes_leading_newline_produces_no_blank_line() {
        let summary = CrashSummary {
            app_notes: Some("\n-L1000-W0000100-T1) DWrite?\nsecond line".to_string()),
            ..Default::default()
        };
        let output = format_annotations(&summary);
        assert_eq!(
            output,
            "annotations:\n  app_notes: -L1000-W0000100-T1) DWrite? second line\n"
        );
        assert!(
            !output.lines().any(|line| line.trim().is_empty()),
            "blank line in:\n{}",
            output
        );
    }

    #[test]
    fn test_compact_annotations_crash_inconsistencies() {
        // Empty is the common case and must not print a key.
        let summary = CrashSummary {
            last_error_value: Some("ERROR_SUCCESS".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_annotations(&summary),
            "annotations:\n  last_error: ERROR_SUCCESS\n"
        );

        let summary = CrashSummary {
            crash_inconsistencies: vec!["A".to_string(), "B".to_string()],
            ..Default::default()
        };
        assert_eq!(
            format_annotations(&summary),
            "annotations:\n  crash_inconsistencies: A, B\n"
        );
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
