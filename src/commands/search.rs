// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::models::SearchParams;
use crate::output::{compact, json, markdown, OutputFormat};
use crate::{Result, SocorroClient};

pub fn execute(client: &SocorroClient, params: SearchParams, format: OutputFormat) -> Result<()> {
    let response = client.search(params)?;

    let output = match format {
        OutputFormat::Compact => compact::format_search(&response),
        OutputFormat::Json => json::format_search(&response)?,
        OutputFormat::Markdown => markdown::format_search(&response),
    };

    print!("{}", output);
    Ok(())
}
