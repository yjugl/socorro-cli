// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use reqwest::StatusCode;
use sha1::{Digest, Sha1};

use crate::models::{CorrelationsResponse, CorrelationsTotals};
use crate::output::{compact, json, markdown, OutputFormat};
use crate::{Error, Result};

const CDN_BASE: &str =
    "https://analysis-output.telemetry.mozilla.org/top-signatures-correlations/data";

pub fn signature_hash(sig: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(sig.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn fetch_totals(client: &reqwest::blocking::Client) -> Result<CorrelationsTotals> {
    let url = format!("{}/all.json.gz", CDN_BASE);
    let response = client.get(&url).send()?;

    match response.status() {
        StatusCode::OK => {
            let text = response.text()?;
            serde_json::from_str(&text)
                .map_err(|e| Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)])))
        }
        _ => Err(Error::Http(response.error_for_status().unwrap_err())),
    }
}

fn fetch_signature_correlations(
    client: &reqwest::blocking::Client,
    signature: &str,
    channel: &str,
) -> Result<CorrelationsResponse> {
    let hash = signature_hash(signature);
    let url = format!("{}/{}/{}.json.gz", CDN_BASE, channel, hash);
    let response = client.get(&url).send()?;

    match response.status() {
        StatusCode::OK => {
            let text = response.text()?;
            serde_json::from_str(&text)
                .map_err(|e| Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)])))
        }
        StatusCode::NOT_FOUND => Err(Error::NotFound(format!(
            "No correlation data for signature \"{}\" on channel \"{}\". \
             Correlations are only available for the top ~200 signatures per channel.",
            signature, channel
        ))),
        _ => Err(Error::Http(response.error_for_status().unwrap_err())),
    }
}

pub fn execute(signature: &str, channel: &str, format: OutputFormat) -> Result<()> {
    let client = reqwest::blocking::Client::builder().gzip(true).build()?;

    let totals = fetch_totals(&client)?;

    if totals.total_for_channel(channel).is_none() {
        return Err(Error::ParseError(format!(
            "Unknown channel \"{}\". Valid channels: release, beta, nightly, esr",
            channel
        )));
    }

    let response = fetch_signature_correlations(&client, signature, channel)?;

    let output = match format {
        OutputFormat::Compact => {
            let summary = response.to_summary(signature, channel, &totals);
            compact::format_correlations(&summary)
        }
        OutputFormat::Json => json::format_correlations(&response)?,
        OutputFormat::Markdown => {
            let summary = response.to_summary(signature, channel, &totals);
            markdown::format_correlations(&summary)
        }
    };

    print!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_hash() {
        assert_eq!(
            signature_hash("UiaNode::ProviderInfo::~ProviderInfo"),
            "4361bb82d8d8c7f34466f8b7589fbd6c920da702"
        );
    }

    #[test]
    fn test_signature_hash_oom() {
        let hash = signature_hash("OOM | small");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 40);
    }
}
