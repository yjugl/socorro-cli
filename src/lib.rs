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

    #[error("Unexpected HTTP status {status} from {url}")]
    UnexpectedStatus { status: u16, url: String },
}

/// Turns a response whose status none of a caller's explicit arms recognised
/// into an [`Error`], without assuming that status is a failure.
///
/// Every HTTP path in this crate matches the statuses it knows how to handle
/// and needs one fallthrough arm for the rest. The obvious spelling of that
/// arm -- calling [`reqwest::blocking::Response::error_for_status`] and taking
/// the error out of its `Result` unconditionally -- is wrong: that method
/// returns `Ok(self)` for anything that is neither a client nor a server
/// error, so a `202`, a `204` or an unfollowed `3xx` unwraps an `Ok` and
/// aborts the process. That is not hypothetical. `crash-pings.mozilla.org`
/// serves **202** for a day whose data is not built yet (probed 2026-09-02:
/// 202 for `2026-09-02`, `2026-09-03` and `2027-01-01`), and the CDN behind
/// `commands::correlations` can do the same.
///
/// So this reads the classification off `error_for_status` itself rather than
/// re-deriving it with `is_client_error() || is_server_error()`: the `Err`
/// branch is exactly the 4xx/5xx case and keeps [`Error::Http`], which carries
/// reqwest's richer context (url, status, source); the `Ok` branch is exactly
/// the set of statuses that need [`Error::UnexpectedStatus`]. The two cannot
/// drift apart if reqwest ever changes what it treats as an error, and there
/// is no panicking path left by construction rather than by argument.
///
/// The reported URL comes from `response.url()`, i.e. the URL reqwest actually
/// fetched -- after any redirect, and with the query string a caller that
/// built its request with `.query(...)` never had in a format string.
pub fn status_error(response: reqwest::blocking::Response) -> Error {
    match response.error_for_status() {
        Err(err) => Error::Http(err),
        Ok(response) => Error::UnexpectedStatus {
            status: response.status().as_u16(),
            url: response.url().to_string(),
        },
    }
}

/// How many bytes of an unparseable response body to quote in
/// [`Error::ParseError`].
///
/// Every fetch in this crate previews the body it failed to deserialize, and
/// the cap is deliberately the same everywhere: one malformed CDN object and
/// one malformed Socorro response then cost a reader the same number of
/// tokens.
///
/// On a `&str` body the cap must be applied through
/// [`truncate_on_char_boundary`] rather than by slicing at the byte offset,
/// because the body is arbitrary bytes off the network and an offset landing
/// inside a multi-byte character panics on the very path that exists to report
/// a problem. Slicing a `&[u8]` at this offset is safe by contrast -- byte
/// slices have no character boundaries -- and `commands::crash_pings` does
/// exactly that on the one path where it already holds the raw bytes,
/// repairing any split sequence with `from_utf8_lossy`.
pub const PREVIEW_BYTES: usize = 200;

/// Returns a prefix of `text` that is at most `max` *bytes* long and always
/// ends on a UTF-8 character boundary.
///
/// This exists for the `Error::ParseError` response preview, which quotes the
/// first couple of hundred bytes of a body the client failed to deserialize.
/// The naive form of that preview, `&text[..text.len().min(200)]`, panics
/// whenever byte `200` lands inside a multi-byte character: 199 ASCII bytes
/// followed by an em dash aborts with `end byte index 200 is not a char
/// boundary; it is inside '—' (bytes 199..202 of string)`. The body is
/// arbitrary bytes off the network, so the naive slice turns a diagnostic
/// into a crash at exactly the moment the diagnostic was wanted.
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

    /// `status_error` takes the URL off `response.url()` rather than from a
    /// caller-supplied string. This pins that down: the reported URL is the one
    /// reqwest actually fetched, query string included -- which the three
    /// hand-built `format!` strings this function replaced did not have.
    #[test]
    fn unhandled_status_reports_the_url_reqwest_actually_fetched() {
        let server = test_server::TestServer::start();
        server.push_response(204, "");
        let requested = format!("{}/ping_data/2026-09-01", server.base_url());
        let response = reqwest::blocking::Client::new()
            .get(&requested)
            .query(&[("signature", "OOM | small")])
            .send()
            .expect("loopback request should complete");

        let err = status_error(response);

        let Error::UnexpectedStatus { status, url } = &err else {
            panic!("expected Error::UnexpectedStatus, got {err:?}");
        };
        assert_eq!(*status, 204);
        assert_eq!(url, &format!("{requested}?signature=OOM+%7C+small"));
    }

    /// The `Err` branch of `error_for_status` is exactly the 4xx/5xx case, so a
    /// genuine failure keeps reqwest's richer error instead of being flattened
    /// into `UnexpectedStatus`.
    #[test]
    fn unhandled_status_keeps_reqwest_error_for_a_server_error() {
        let server = test_server::TestServer::start();
        server.push_response(503, "try later");
        let response = reqwest::blocking::Client::new()
            .get(server.base_url())
            .send()
            .expect("loopback request should complete");

        let err = status_error(response);

        let Error::Http(source) = &err else {
            panic!("expected Error::Http, got {err:?}");
        };
        assert_eq!(source.status().map(|s| s.as_u16()), Some(503));
    }

    #[test]
    fn unhandled_status_keeps_reqwest_error_for_a_client_error() {
        let server = test_server::TestServer::start();
        server.push_response(418, "teapot");
        let response = reqwest::blocking::Client::new()
            .get(server.base_url())
            .send()
            .expect("loopback request should complete");

        let err = status_error(response);

        let Error::Http(source) = &err else {
            panic!("expected Error::Http, got {err:?}");
        };
        assert_eq!(source.status().map(|s| s.as_u16()), Some(418));
    }

    #[test]
    fn unexpected_status_names_the_status_and_the_url() {
        let err = Error::UnexpectedStatus {
            status: 202,
            url: "https://crash-pings.mozilla.org/data/2026-09-01.json".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Unexpected HTTP status 202 from \
             https://crash-pings.mozilla.org/data/2026-09-01.json"
        );
    }
}
