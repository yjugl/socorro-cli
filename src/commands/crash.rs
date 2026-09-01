// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::models::{CrashSummary, ModulesMode};
use crate::output::{OutputFormat, compact, json, markdown};
use crate::{Result, SocorroClient};

fn extract_crash_id(input: &str) -> &str {
    if input.starts_with("http://") || input.starts_with("https://") {
        // Handle trailing slashes by filtering empty segments
        input.rsplit('/').find(|s| !s.is_empty()).unwrap_or(input)
    } else {
        input
    }
}

/// Appends an extra section to a crash report body, separated by exactly one
/// blank line.
///
/// Neither of the two variables here is constant, so both are handled instead
/// of assumed. `format_crash`'s trailing newline count varies with what it
/// emitted last: the markdown `## Details` list and an `--all-threads` code
/// fence both end with a blank line, while a module table or a single stack
/// trace ends with just one newline. And the two annotation formatters differ
/// at the front: `markdown::format_annotations` starts with its own newline,
/// `compact::format_annotations` starts directly with `annotations:`.
///
/// Trimming both sides makes the result independent of either, so neither
/// formatter has to be changed and neither one's trailing or leading whitespace
/// has to be relied on.
fn append_section(body: &mut String, section: &str) {
    while body.ends_with('\n') {
        body.pop();
    }
    body.push_str("\n\n");
    body.push_str(section.trim_start_matches('\n'));
}

/// Renders the compact crash report, optionally followed by the annotations
/// section.
fn compose_compact(summary: &CrashSummary, modules_mode: ModulesMode, annotations: bool) -> String {
    let mut output = compact::format_crash(summary, modules_mode);
    if annotations {
        append_section(&mut output, &compact::format_annotations(summary));
    }
    output
}

/// Renders the markdown crash report, optionally followed by the annotations
/// section.
fn compose_markdown(
    summary: &CrashSummary,
    modules_mode: ModulesMode,
    annotations: bool,
) -> String {
    let mut output = markdown::format_crash(summary, modules_mode);
    if annotations {
        append_section(&mut output, &markdown::format_annotations(summary));
    }
    output
}

/// Whether to send the API token when fetching this crash.
///
/// Security invariant (see CLAUDE.md, "JSON Crash Output Skips Auth Token"):
/// when the output will be JSON, the API token is deliberately not sent, so the
/// server strips protected fields (registers, `mac_boot_args`, ...) from
/// `json_dump` on its side. Both JSON-shaped paths are covered: `--full` and
/// `--format json`. Compact and markdown output only ever surfaces public
/// sub-fields via `to_summary()`, so those paths authenticate to get the higher
/// rate limit.
///
/// This is kept as a pure function of the output shape purely so the matrix of
/// (`full`, `format`) can be enumerated in an offline test; `execute()` does
/// network I/O and cannot be unit-tested. Do not inline it back into the call
/// site.
fn should_use_auth(full: bool, format: OutputFormat) -> bool {
    // `--full` is a raw passthrough of the server's response whatever
    // `--format` says, so it is JSON-shaped for every format.
    if full {
        return false;
    }
    // Matched exhaustively, with deliberately no wildcard arm: a new
    // `OutputFormat` variant must break the build here. Written as
    // `format != OutputFormat::Json` this function would instead hand a new
    // variant the dangerous default (`true`, send the token), which for a
    // JSON-shaped variant is exactly the leak this invariant exists to prevent.
    match format {
        OutputFormat::Json => false,
        OutputFormat::Compact | OutputFormat::Markdown => true,
    }
}

// The parameters mirror the `crash` subcommand's flags one-for-one; bundling
// them into a params struct (as `search::execute` does) is a refactor of its
// own and would not make any call site clearer today.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    client: &SocorroClient,
    crash_id: &str,
    depth: usize,
    full: bool,
    all_threads: bool,
    modules_mode: ModulesMode,
    annotations: bool,
    format: OutputFormat,
) -> Result<()> {
    let crash_id = extract_crash_id(crash_id);
    let use_auth = should_use_auth(full, format);

    // JSON output is a raw passthrough of the API response: deserializing into
    // `ProcessedCrash` first would silently drop every key the struct does not
    // declare. The body is still parsed before being emitted, so a malformed
    // response yields `Error::ParseError` rather than garbage on stdout.
    // `--annotations` is ignored here because the passthrough already carries
    // every annotation.
    if full || format == OutputFormat::Json {
        let value = client.get_crash_raw(crash_id, use_auth)?;
        print!("{}", json::format_crash_raw(&value)?);
        return Ok(());
    }

    let crash = client.get_crash(crash_id, use_auth)?;

    if modules_mode == ModulesMode::ThirdParty {
        let os = crash.os_name.as_deref().unwrap_or("");
        if !os.starts_with("Windows") {
            return Err(crate::Error::UnsupportedOption(
                "--modules third-party is only supported on Windows crashes (cert_subject is not available on other platforms)".to_string(),
            ));
        }
    }

    let summary = crash.to_summary(depth, all_threads);
    let output = match format {
        OutputFormat::Compact => compose_compact(&summary, modules_mode, annotations),
        OutputFormat::Markdown => compose_markdown(&summary, modules_mode, annotations),
        // Handled by the raw passthrough above.
        OutputFormat::Json => unreachable!("JSON output returns early via the raw passthrough"),
    };

    print!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StackFrame;
    // For `OutputFormat::value_variants()`: the derived enumeration of every
    // format, used to keep the auth matrix below variant-complete.
    use clap::ValueEnum;

    #[test]
    fn test_extract_crash_id_bare_id() {
        let id = "247653e8-7a18-4836-97d1-42a720260120";
        assert_eq!(extract_crash_id(id), id);
    }

    #[test]
    fn test_extract_crash_id_from_report_url() {
        let url =
            "https://crash-stats.mozilla.org/report/index/247653e8-7a18-4836-97d1-42a720260120";
        assert_eq!(
            extract_crash_id(url),
            "247653e8-7a18-4836-97d1-42a720260120"
        );
    }

    #[test]
    fn test_extract_crash_id_from_url_with_trailing_slash() {
        let url =
            "https://crash-stats.mozilla.org/report/index/247653e8-7a18-4836-97d1-42a720260120/";
        assert_eq!(
            extract_crash_id(url),
            "247653e8-7a18-4836-97d1-42a720260120"
        );
    }

    fn sample_summary() -> CrashSummary {
        CrashSummary {
            crash_id: "b98bbb81-3ff6-4825-991f-6a0b30260901".to_string(),
            signature: "OOM | small".to_string(),
            product: "Firefox".to_string(),
            version: "147.0".to_string(),
            platform: "Windows 10".to_string(),
            app_notes: Some("FP(D00-L1000-W0000100-T1)".to_string()),
            shutdown_progress: Some("profile-before-change".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_compose_compact_without_annotations_is_unchanged() {
        let summary = sample_summary();
        let plain = compact::format_crash(&summary, ModulesMode::Stack);
        assert_eq!(compose_compact(&summary, ModulesMode::Stack, false), plain);
        assert!(!plain.contains("annotations:"));
    }

    #[test]
    fn test_compose_compact_separates_annotations_with_one_blank_line() {
        let output = compose_compact(&sample_summary(), ModulesMode::Stack, true);
        assert!(
            output.contains("\n\nannotations:"),
            "expected a blank line before the annotations section, got: {:?}",
            output
        );
        assert!(
            !output.contains("\n\n\nannotations:"),
            "expected exactly one blank line before the annotations section, got: {:?}",
            output
        );
    }

    #[test]
    fn test_compose_markdown_does_not_double_the_blank_line() {
        let output = compose_markdown(&sample_summary(), ModulesMode::Stack, true);
        assert!(
            output.contains("\n\n## Annotations"),
            "expected a blank line before the Annotations heading, got: {:?}",
            output
        );
        assert!(
            !output.contains("\n\n\n## Annotations"),
            "expected exactly one blank line before the Annotations heading, got: {:?}",
            output
        );
    }

    /// A summary whose report body ends with a stack trace, i.e. with a single
    /// trailing newline rather than the blank line the `## Details` list leaves.
    fn sample_summary_with_stack() -> CrashSummary {
        CrashSummary {
            crashing_thread_name: Some("GeckoMain".to_string()),
            frames: vec![StackFrame {
                frame: 0,
                function: Some("mozilla::Foo".to_string()),
                file: Some("Foo.cpp".to_string()),
                line: Some(42),
                module: None,
                offset: None,
            }],
            ..sample_summary()
        }
    }

    #[test]
    fn test_compose_separator_is_normalized_for_both_body_shapes() {
        // The report body's trailing newline count differs between these two
        // shapes, so both must still yield exactly one blank line.
        for summary in [sample_summary(), sample_summary_with_stack()] {
            let compact_out = compose_compact(&summary, ModulesMode::Stack, true);
            assert!(compact_out.contains("\n\nannotations:"));
            assert!(!compact_out.contains("\n\n\nannotations:"));

            let markdown_out = compose_markdown(&summary, ModulesMode::Stack, true);
            assert!(markdown_out.contains("\n\n## Annotations"));
            assert!(!markdown_out.contains("\n\n\n## Annotations"));
        }
    }

    #[test]
    fn test_append_section_normalizes_both_sides() {
        // Every case must land on exactly one blank line between the two parts,
        // whatever the body ends with and whatever the section starts with.
        let cases = [
            // (body, section)
            ("body\n", "section\n"),     // one newline, no leading newline
            ("body\n", "\nsection\n"),   // one newline, leading newline
            ("body\n\n", "section\n"),   // trailing blank line, no leading newline
            ("body\n\n", "\nsection\n"), // trailing blank line, leading newline
            ("body\n\n\n\n", "\n\nsection\n"), // excess on both sides
            ("body", "section\n"),       // no trailing newline at all
        ];
        for (body, section) in cases {
            let mut got = body.to_string();
            append_section(&mut got, section);
            assert_eq!(
                got, "body\n\nsection\n",
                "body {:?} + section {:?} was not normalized",
                body, section
            );
        }
    }

    #[test]
    fn test_compose_markdown_without_annotations_is_unchanged() {
        let summary = sample_summary();
        let plain = markdown::format_crash(&summary, ModulesMode::Stack);
        assert_eq!(compose_markdown(&summary, ModulesMode::Stack, false), plain);
        assert!(!plain.contains("## Annotations"));
    }

    /// A protected-field leak guard, not a formatting nit: if this fails, the
    /// `crash` command is about to send the API token on a code path whose
    /// output is a raw passthrough of the server's response. A token carrying
    /// `view_pii` would then make the server return protected `json_dump`
    /// sub-objects (registers, `mac_boot_args`, ...) and the tool would print
    /// them verbatim. See CLAUDE.md, "JSON Crash Output Skips Auth Token".
    #[test]
    fn test_json_shaped_output_never_authenticates_leak_guard() {
        // Every way of asking for JSON, with and without --full.
        for full in [true, false] {
            assert!(
                !should_use_auth(full, OutputFormat::Json),
                "--format json (full={}) must be fetched WITHOUT a token",
                full
            );
        }
        // `--full` is a raw passthrough for every format, so it is JSON-shaped
        // regardless of what --format says. Iterating clap's generated
        // `value_variants()` rather than a hand-written list means a newly
        // added format is actually covered here.
        for &format in OutputFormat::value_variants() {
            assert!(
                !should_use_auth(true, format),
                "--full ({:?}) must be fetched WITHOUT a token",
                format
            );
        }
    }

    #[test]
    fn test_should_use_auth_full_matrix() {
        // The complete (full, format) matrix, enumerated rather than
        // spot-checked. Auth is sent for exactly two of the six cases: the
        // summarized compact and markdown reports, which `to_summary()` limits
        // to public sub-fields.
        let cases = [
            (false, OutputFormat::Compact, true),
            (false, OutputFormat::Markdown, true),
            (false, OutputFormat::Json, false),
            (true, OutputFormat::Compact, false),
            (true, OutputFormat::Markdown, false),
            (true, OutputFormat::Json, false),
        ];
        for (full, format, expected) in cases {
            assert_eq!(
                should_use_auth(full, format),
                expected,
                "should_use_auth(full={}, format={:?}) should be {}",
                full,
                format,
                expected
            );
        }
        // Guard the matrix itself: every variant of `OutputFormat` must appear
        // above. A hand-written list here would never see a fourth format, so
        // this iterates clap's generated `value_variants()`, which does; adding
        // a format therefore fails this test until its two rows are added. (The
        // primary protection is the exhaustive `match` in `should_use_auth`,
        // which fails to compile; this only keeps the table honest.)
        for &format in OutputFormat::value_variants() {
            for full in [true, false] {
                assert!(
                    cases.iter().any(|&(f, fmt, _)| f == full && fmt == format),
                    "matrix is missing (full={}, format={:?})",
                    full,
                    format
                );
            }
        }
    }

    /// The non-`--full` compact and markdown paths must keep sending the token:
    /// dropping it is not a security problem but it costs the authenticated
    /// rate limit, which is the whole reason the token is configured.
    #[test]
    fn test_summarized_output_still_authenticates_for_rate_limit() {
        assert!(should_use_auth(false, OutputFormat::Compact));
        assert!(should_use_auth(false, OutputFormat::Markdown));
    }

    #[test]
    fn test_extract_crash_id_http_url() {
        let url =
            "http://crash-stats.mozilla.org/report/index/247653e8-7a18-4836-97d1-42a720260120";
        assert_eq!(
            extract_crash_id(url),
            "247653e8-7a18-4836-97d1-42a720260120"
        );
    }
}
