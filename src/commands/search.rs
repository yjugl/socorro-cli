use crate::{Result, SocorroClient};
use crate::models::SearchParams;
use crate::output::{OutputFormat, compact, json, markdown};

pub fn execute(
    client: &SocorroClient,
    params: SearchParams,
    format: OutputFormat,
) -> Result<()> {
    let response = client.search(params)?;

    let output = match format {
        OutputFormat::Compact => compact::format_search(&response),
        OutputFormat::Json => json::format_search(&response)?,
        OutputFormat::Markdown => markdown::format_search(&response),
    };

    print!("{}", output);
    Ok(())
}
