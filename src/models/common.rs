// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Deserializer, Serialize};

pub fn deserialize_string_or_number<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }))
}

pub fn deserialize_string_or_number_required<'de, D>(
    deserializer: D,
) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    #[serde(default)]
    pub frame: u32,
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub module: Option<String>,
    pub offset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub filename: String,
    pub debug_file: Option<String>,
    pub debug_id: Option<String>,
    pub code_id: Option<String>,
    pub version: Option<String>,
    pub cert_subject: Option<String>,
}

impl ModuleInfo {
    /// Returns true if this module is not signed by Mozilla or Microsoft.
    /// On Windows, `cert_subject` is populated from Authenticode signatures.
    /// On Linux/macOS, `cert_subject` is always null, so all modules are considered third-party.
    pub fn is_third_party(&self) -> bool {
        match &self.cert_subject {
            Some(cert) => !cert.starts_with("Mozilla ") && !cert.starts_with("Microsoft "),
            // No cert = unsigned = third-party
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_with_cert(cert: Option<&str>) -> ModuleInfo {
        ModuleInfo {
            filename: "test.dll".to_string(),
            debug_file: None,
            debug_id: None,
            code_id: None,
            version: None,
            cert_subject: cert.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_is_third_party_mozilla_corporation() {
        assert!(!module_with_cert(Some("Mozilla Corporation")).is_third_party());
    }

    #[test]
    fn test_is_third_party_microsoft_windows() {
        assert!(!module_with_cert(Some("Microsoft Windows")).is_third_party());
    }

    #[test]
    fn test_is_third_party_microsoft_corporation() {
        assert!(!module_with_cert(Some("Microsoft Corporation")).is_third_party());
    }

    #[test]
    fn test_is_third_party_microsoft_prefix() {
        assert!(!module_with_cert(Some("Microsoft Windows Production PCA 2011")).is_third_party());
    }

    #[test]
    fn test_is_third_party_trend_micro() {
        assert!(module_with_cert(Some("Trend Micro, Inc.")).is_third_party());
    }

    #[test]
    fn test_is_third_party_unsigned() {
        assert!(module_with_cert(None).is_third_party());
    }
}
