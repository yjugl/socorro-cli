// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::output::{OutputFormat, compact, json, markdown};
use crate::{Result, SocorroClient};

pub fn execute(
    client: &SocorroClient,
    signatures: &[String],
    bug_ids: &[u64],
    format: OutputFormat,
) -> Result<()> {
    let response = if !signatures.is_empty() {
        client.get_bugs(signatures)?
    } else {
        client.get_signatures_by_bugs(bug_ids)?
    };

    let output = match format {
        OutputFormat::Compact => {
            let summary = response.to_summary();
            compact::format_bugs(&summary)
        }
        OutputFormat::Json => json::format_bugs(&response)?,
        OutputFormat::Markdown => {
            let summary = response.to_summary();
            markdown::format_bugs(&summary)
        }
    };

    print!("{}", output);
    Ok(())
}
