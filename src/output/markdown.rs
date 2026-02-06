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

    output.push_str(&"# Crash Report\n\n".to_string());
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
            output.push_str(&format!("- **Crash Reason:** {} at `{}`{}\n", reason, addr_str, addr_desc));
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

    output.push_str(&format!("- **Product:** {} {}\n", summary.product, summary.version));
    output.push_str(&format!("- **Platform:** {}{}\n\n", summary.platform, device_info));

    if !summary.all_threads.is_empty() {
        output.push_str("## All Threads\n\n");
        for thread in &summary.all_threads {
            let thread_name = thread.thread_name.as_deref().unwrap_or("unknown");
            let crash_marker = if thread.is_crashing { " **[CRASHING]**" } else { "" };
            output.push_str(&format!("### Thread {} ({}){}\n\n", thread.thread_index, thread_name, crash_marker));
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

    output
}

pub fn format_search(response: &SearchResponse) -> String {
    let mut output = String::new();

    output.push_str(&"# Search Results\n\n".to_string());
    output.push_str(&format!("Found **{}** crashes\n\n", response.total));

    if !response.hits.is_empty() {
        output.push_str("## Crashes\n\n");
        output.push_str("| Crash ID | Product | Version | Platform | Signature |\n");
        output.push_str("|----------|---------|---------|----------|----------|\n");

        for hit in &response.hits {
            let platform = hit.os_name.as_deref().unwrap_or("Unknown");
            output.push_str(&format!("| {} | {} | {} | {} | {} |\n",
                &hit.uuid[..8],
                hit.product,
                hit.version,
                platform,
                hit.signature
            ));
        }
        output.push('\n');
    }

    if !response.facets.is_empty() {
        output.push_str("## Aggregations\n\n");
        for (field, buckets) in &response.facets {
            output.push_str(&format!("### {}\n\n", field));
            for bucket in buckets {
                output.push_str(&format!("- **{}**: {} crashes\n", bucket.term, bucket.count));
            }
            output.push('\n');
        }
    }

    output
}
