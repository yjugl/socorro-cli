// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Crash annotation types.
//!
//! Socorro returns most annotations as plain scalars, but a few carry structure
//! inside a string. `async_shutdown_timeout` is a JSON document embedded in a
//! JSON string; [`AsyncShutdownTimeout`] parses it when it has the expected
//! shape and falls back to the verbatim string when it does not, so no data is
//! ever dropped.

use serde::{Deserialize, Deserializer};

/// One entry of `async_shutdown_timeout.conditions`: a shutdown blocker that
/// was still pending when the shutdown phase timed out.
#[derive(Debug, Clone, Deserialize)]
pub struct ShutdownCondition {
    pub name: String,
    #[serde(default)]
    pub filename: Option<String>,
    /// The JSON key is camelCase (`lineNumber`).
    #[serde(default, rename = "lineNumber")]
    pub line_number: Option<u64>,
    /// Blocker-defined payload. Sometimes an object
    /// (`{"pending": 1}`), sometimes a bare string (`"(none)"`), sometimes
    /// absent — hence `Value`.
    #[serde(default)]
    pub state: serde_json::Value,
}

impl ShutdownCondition {
    /// A one-line rendering of `state`, or `None` when there is nothing to
    /// show. Strings are returned as-is; anything else is compact JSON.
    pub fn state_display(&self) -> Option<String> {
        match &self.state {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
}

/// The structured content of the `async_shutdown_timeout` blob.
#[derive(Debug, Clone, Deserialize)]
pub struct AsyncShutdownTimeoutData {
    /// The shutdown phase that timed out, e.g. `profile-before-change`.
    pub phase: String,
    #[serde(default)]
    pub conditions: Vec<ShutdownCondition>,
}

/// The `async_shutdown_timeout` annotation, parsed when possible.
#[derive(Debug, Clone)]
pub enum AsyncShutdownTimeout {
    Parsed(AsyncShutdownTimeoutData),
    /// The embedded string was not JSON, or not of the expected shape. Kept
    /// verbatim so a formatter can emit it unchanged rather than lose it.
    Raw(String),
}

impl AsyncShutdownTimeout {
    /// Parse the embedded JSON string. Never fails: an unparseable or
    /// unexpected payload becomes [`AsyncShutdownTimeout::Raw`].
    pub fn parse(raw: &str) -> Self {
        // Require a JSON object explicitly: serde happily deserializes a
        // struct from a JSON *array* by position, which would turn an
        // unexpected payload like `["profile-before-change"]` into a
        // plausible-looking parse instead of a verbatim fallback.
        let parsed = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .filter(|v| v.is_object())
            .and_then(|v| serde_json::from_value::<AsyncShutdownTimeoutData>(v).ok());

        match parsed {
            Some(data) => AsyncShutdownTimeout::Parsed(data),
            None => AsyncShutdownTimeout::Raw(raw.trim().to_string()),
        }
    }
}

/// Accept an integer that the API may send as a number or as a numeric string.
/// Anything else (including a non-numeric string) yields `None` rather than
/// failing the whole crash deserialization.
pub fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }))
}

/// Accept a boolean that the API may send as a bool, a 0/1 number, or a
/// string. Unrecognized values yield `None` rather than a hard error.
pub fn deserialize_optional_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::Bool(b) => Some(b),
        serde_json::Value::Number(n) => n.as_i64().map(|n| n != 0),
        serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped after the live `async_shutdown_timeout` blob of crash
    /// b98bbb81-3ff6-4825-991f-6a0b30260901 (1151 characters): three
    /// conditions, one with an object `state` and a string `stack`, one with an
    /// array `stack`, one with a string `state`.
    const BLOB: &str = r#"{
        "phase": "profile-before-change",
        "conditions": [
            {
                "name": "ServiceWorkerRegistrar: Flushing data",
                "state": {"saveDataRunnableDispatched": false, "shuttingDown": false},
                "filename": "..\\..\\..\\..\\checkouts\\gecko\\dom\\serviceworkers\\ServiceWorkerRegistrar.cpp",
                "lineNumber": 1566,
                "stack": "ServiceWorkerRegistrar: Flushing data"
            },
            {
                "name": "ASRouterStorage: flush pending writes",
                "state": {"pending": 1},
                "filename": "resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs",
                "lineNumber": 50,
                "stack": ["resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs:createStorage:50"]
            },
            {
                "name": "ShieldRecipeClient: Cleaning up",
                "state": "(none)",
                "filename": "resource://normandy/lib/CleanupManager.sys.mjs",
                "lineNumber": 39,
                "stack": ["resource://normandy/lib/CleanupManager.sys.mjs:cleanup:39"]
            }
        ]
    }"#;

    fn parsed(raw: &str) -> AsyncShutdownTimeoutData {
        match AsyncShutdownTimeout::parse(raw) {
            AsyncShutdownTimeout::Parsed(data) => data,
            AsyncShutdownTimeout::Raw(raw) => panic!("expected a parsed blob, got raw: {}", raw),
        }
    }

    #[test]
    fn test_parse_async_shutdown_timeout_phase_and_conditions() {
        let data = parsed(BLOB);
        assert_eq!(data.phase, "profile-before-change");
        assert_eq!(data.conditions.len(), 3);
    }

    #[test]
    fn test_parse_async_shutdown_timeout_filename_and_line() {
        let data = parsed(BLOB);
        let first = &data.conditions[0];
        assert_eq!(first.name, "ServiceWorkerRegistrar: Flushing data");
        assert!(
            first
                .filename
                .as_deref()
                .unwrap()
                .ends_with("ServiceWorkerRegistrar.cpp"),
            "unexpected filename: {:?}",
            first.filename
        );
        assert_eq!(first.line_number, Some(1566));
    }

    #[test]
    fn test_parse_async_shutdown_timeout_object_state() {
        let data = parsed(BLOB);
        let display = data.conditions[1].state_display().unwrap();
        assert!(display.contains("\"pending\":1"), "got {}", display);
    }

    #[test]
    fn test_parse_async_shutdown_timeout_string_state() {
        // A string-valued `state` must not break parsing of the whole blob.
        let data = parsed(BLOB);
        assert_eq!(
            data.conditions[2].state_display(),
            Some("(none)".to_string())
        );
    }

    #[test]
    fn test_parse_async_shutdown_timeout_polymorphic_stack() {
        // `stack` is a string in the first condition and an array in the
        // others; both must deserialize.
        let data = parsed(BLOB);
        assert_eq!(
            data.conditions[0].name,
            "ServiceWorkerRegistrar: Flushing data"
        );
        assert_eq!(
            data.conditions[1].name,
            "ASRouterStorage: flush pending writes"
        );
    }

    #[test]
    fn test_parse_async_shutdown_timeout_missing_state() {
        let data = parsed(r#"{"phase": "quit-application", "conditions": [{"name": "Blocker"}]}"#);
        assert_eq!(data.conditions[0].state_display(), None);
        assert_eq!(data.conditions[0].filename, None);
        assert_eq!(data.conditions[0].line_number, None);
    }

    #[test]
    fn test_parse_async_shutdown_timeout_no_conditions() {
        let data = parsed(r#"{"phase": "quit-application"}"#);
        assert_eq!(data.phase, "quit-application");
        assert!(data.conditions.is_empty());
    }

    #[test]
    fn test_parse_async_shutdown_timeout_malformed_falls_back_to_raw() {
        let raw = "not json at all {";
        match AsyncShutdownTimeout::parse(raw) {
            AsyncShutdownTimeout::Raw(kept) => assert_eq!(kept, raw),
            AsyncShutdownTimeout::Parsed(data) => panic!("unexpectedly parsed: {:?}", data),
        }
    }

    #[test]
    fn test_parse_async_shutdown_timeout_unexpected_shape_falls_back_to_raw() {
        // Valid JSON, but not the expected object shape: keep it verbatim
        // rather than lose it.
        let raw = r#"["profile-before-change"]"#;
        match AsyncShutdownTimeout::parse(raw) {
            AsyncShutdownTimeout::Raw(kept) => assert_eq!(kept, raw),
            AsyncShutdownTimeout::Parsed(data) => panic!("unexpectedly parsed: {:?}", data),
        }
    }
}
