use crate::models::{CrashSummary, SearchResponse, StackFrame};

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

pub fn format_crash(summary: &CrashSummary) -> String {
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

    output.push_str(&format!("product: {} {} ({}{})\n",
        summary.product,
        summary.version,
        summary.platform,
        device_info
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
            let crash_marker = if thread.is_crashing { " [CRASHING]" } else { "" };
            output.push_str(&format!("stack[thread {}:{}{}]:\n", thread.thread_index, thread_name, crash_marker));

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

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CrashSummary, CrashHit, FacetBucket, ThreadSummary};
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
            frames: vec![
                StackFrame {
                    frame: 0,
                    function: Some("EnsureTimeStretcher".to_string()),
                    file: Some("AudioDecoderInputTrack.cpp".to_string()),
                    line: Some(624),
                    module: None,
                    offset: None,
                },
            ],
            all_threads: vec![],
        }
    }

    #[test]
    fn test_format_crash_header() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary);

        assert!(output.contains("CRASH 247653e8-7a18-4836-97d1-42a720260120"));
        assert!(output.contains("sig: mozilla::AudioDecoderInputTrack::EnsureTimeStretcher"));
    }

    #[test]
    fn test_format_crash_reason_with_null_ptr() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary);

        assert!(output.contains("reason: SIGSEGV @ 0x0 (null ptr)"));
    }

    #[test]
    fn test_format_crash_moz_reason() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary);

        assert!(output.contains("moz_reason: MOZ_RELEASE_ASSERT(mTimeStretcher->Init())"));
    }

    #[test]
    fn test_format_crash_product_with_device() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary);

        assert!(output.contains("product: Fenix 147.0.1 (Android 36, SM-S918B 36)"));
    }

    #[test]
    fn test_format_crash_stack_trace() {
        let summary = sample_crash_summary();
        let output = format_crash(&summary);

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
        let output = format_crash(&summary);

        assert!(output.contains("stack[thread 0:MainThread]:"));
        assert!(output.contains("stack[thread 1:GraphRunner [CRASHING]]:"));
    }

    #[test]
    fn test_format_search_basic() {
        let response = SearchResponse {
            total: 42,
            hits: vec![
                CrashHit {
                    uuid: "247653e8-7a18-4836-97d1-42a720260120".to_string(),
                    date: "2024-01-15".to_string(),
                    signature: "mozilla::SomeFunction".to_string(),
                    product: "Firefox".to_string(),
                    version: "120.0".to_string(),
                    platform: Some("Windows".to_string()),
                    build_id: Some("20240115103000".to_string()),
                    release_channel: Some("release".to_string()),
                },
            ],
            facets: HashMap::new(),
        };
        let output = format_search(&response);

        assert!(output.contains("FOUND 42 crashes"));
        assert!(output.contains("247653e8"));
        assert!(output.contains("Firefox 120.0"));
        assert!(output.contains("Windows"));
        assert!(output.contains("mozilla::SomeFunction"));
    }

    #[test]
    fn test_format_search_with_facets() {
        let mut facets = HashMap::new();
        facets.insert("version".to_string(), vec![
            FacetBucket { term: "120.0".to_string(), count: 50 },
            FacetBucket { term: "119.0".to_string(), count: 30 },
        ]);
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
}

pub fn format_search(response: &SearchResponse) -> String {
    let mut output = String::new();

    output.push_str(&format!("FOUND {} crashes\n\n", response.total));

    for hit in &response.hits {
        let platform = hit.platform.as_deref().unwrap_or("?");
        let channel = hit.release_channel.as_deref().unwrap_or("?");
        let build = hit.build_id.as_deref().unwrap_or("?");
        output.push_str(&format!("{} | {} {} | {} | {} | {} | {}\n",
            hit.uuid,
            hit.product,
            hit.version,
            platform,
            channel,
            build,
            hit.signature
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
