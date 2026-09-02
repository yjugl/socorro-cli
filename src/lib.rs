// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod auth;
pub mod cache;
pub mod client;
pub mod commands;
pub mod models;
pub mod output;

/// Test-only HTTP server for exercising the crate's HTTP error paths offline.
/// Never compiled into a shipping build.
#[cfg(test)]
pub mod test_server;

pub use auth::{get_token, has_token};
pub use client::SocorroClient;
pub use models::*;
pub use output::OutputFormat;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Crash not found: {0}")]
    NotFound(String),

    #[error(
        "Rate limited. Ask a human to run 'socorro-cli auth login' to set an API token that has no permissions attached to it"
    )]
    RateLimited,

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("Invalid crash ID format: {0}")]
    InvalidCrashId(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("{0}")]
    UnsupportedOption(String),
}

/// Returns a prefix of `text` that is at most `max` *bytes* long and always
/// ends on a UTF-8 character boundary.
///
/// This exists for the `Error::ParseError` response preview, which quotes the
/// first couple of hundred bytes of a body the client failed to deserialize.
/// The naive form of that preview, `&text[..text.len().min(200)]`, panics
/// whenever byte `200` lands inside a multi-byte character: 199 ASCII bytes
/// followed by an em dash aborts with `byte index 200 is not a char boundary;
/// it is inside '\u{2014}' (bytes 199..202)`. The body is arbitrary bytes off
/// the network, so the naive slice turns a diagnostic into a crash at exactly
/// the moment the diagnostic was wanted.
///
/// This never panics, for any input and any `max`. When the cap falls inside a
/// character the prefix is shortened to the preceding boundary, so the result
/// can be up to three bytes shorter than `max`.
pub fn truncate_on_char_boundary(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    // `is_char_boundary(0)` is always true, so this terminates at or above 0.
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_returns_input_shorter_than_the_cap_whole() {
        assert_eq!(truncate_on_char_boundary("short", 200), "short");
    }

    #[test]
    fn truncate_returns_input_exactly_at_the_cap_whole() {
        let text = "a".repeat(200);
        let truncated = truncate_on_char_boundary(&text, 200);
        assert_eq!(truncated.len(), 200);
        assert_eq!(truncated, text);
    }

    #[test]
    fn truncate_backs_up_when_the_cap_lands_mid_character() {
        // The exact shape that panics with `&text[..200]`: 199 ASCII bytes
        // then a three-byte em dash spanning bytes 199..202.
        let text = "a".repeat(199) + "\u{2014}";
        assert_eq!(text.len(), 202);
        let truncated = truncate_on_char_boundary(&text, 200);
        assert_eq!(truncated.len(), 199);
        assert_eq!(truncated, "a".repeat(199));
    }

    #[test]
    fn truncate_handles_an_all_multibyte_input() {
        // Four-byte characters; a cap of 10 must fall back to 8.
        let text = "\u{1f600}\u{1f600}\u{1f600}";
        assert_eq!(text.len(), 12);
        let truncated = truncate_on_char_boundary(text, 10);
        assert_eq!(truncated.len(), 8);
        assert_eq!(truncated, "\u{1f600}\u{1f600}");
    }

    #[test]
    fn truncate_with_a_zero_cap_returns_empty() {
        assert_eq!(truncate_on_char_boundary("\u{2014}abc", 0), "");
        assert_eq!(truncate_on_char_boundary("abc", 0), "");
    }

    #[test]
    fn truncate_handles_an_empty_input() {
        assert_eq!(truncate_on_char_boundary("", 200), "");
        assert_eq!(truncate_on_char_boundary("", 0), "");
    }
}
