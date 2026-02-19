// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::output::{compact, json, markdown, OutputFormat};
use crate::{Result, SocorroClient};

fn extract_crash_id(input: &str) -> &str {
    if input.starts_with("http://") || input.starts_with("https://") {
        // Handle trailing slashes by filtering empty segments
        input.rsplit('/').find(|s| !s.is_empty()).unwrap_or(input)
    } else {
        input
    }
}

pub fn execute(
    client: &SocorroClient,
    crash_id: &str,
    depth: usize,
    full: bool,
    all_threads: bool,
    format: OutputFormat,
) -> Result<()> {
    let crash_id = extract_crash_id(crash_id);
    let use_auth = !full && format != OutputFormat::Json;
    let crash = client.get_crash(crash_id, use_auth)?;

    let output = if full {
        json::format_crash(&crash)?
    } else {
        match format {
            OutputFormat::Compact => {
                let summary = crash.to_summary(depth, all_threads);
                compact::format_crash(&summary)
            }
            OutputFormat::Json => json::format_crash(&crash)?,
            OutputFormat::Markdown => {
                let summary = crash.to_summary(depth, all_threads);
                markdown::format_crash(&summary)
            }
        }
    };

    print!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
