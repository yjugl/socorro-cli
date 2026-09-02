// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::models::bugs::BugsResponse;
use crate::models::{ProcessedCrash, SearchParams, SearchResponse};
use crate::{Error, PREVIEW_BYTES, Result, auth, status_error, truncate_on_char_boundary};
use reqwest::StatusCode;
use reqwest::blocking::Client;

/// Push a SuperSearch filter parameter onto `query_params`.
///
/// The SuperSearch API has two kinds of filter fields:
///   - **String fields** (signature, proto_signature, platform_version, process_type):
///     The API default (no prefix) does a word-level match, NOT exact match.
///     We prepend `=` for exact match, unless the user already provided an
///     operator prefix (~, $, ^, !, @, etc.).
///   - **Enum fields** (product, version, platform, cpu_arch, release_channel, …):
///     The API default already does exact match.  Prepending `=` silently
///     returns 0 results.  Values are passed through unchanged.
///
/// This function decides which behaviour to apply based on `field`.
/// When adding a new filter field, check its type in the SuperSearch API docs
/// (https://crash-stats.mozilla.org/documentation/supersearch/api/) and add it
/// to STRING_FIELDS if it is a "string" type.
fn push_filter(query_params: &mut Vec<(&str, String)>, field: &'static str, value: String) {
    /// Fields typed "string" in the SuperSearch API.
    /// Verify against https://crash-stats.mozilla.org/documentation/supersearch/api/
    const STRING_FIELDS: &[&str] = &[
        "signature",
        "proto_signature",
        "platform_version",
        "process_type",
    ];

    if STRING_FIELDS.contains(&field) {
        query_params.push((field, exact_match_default(value)));
    } else {
        query_params.push((field, value));
    }
}

/// Prepend `=` to make the Socorro SuperSearch API perform an exact match,
/// unless the value already has a SuperSearch operator prefix.
/// See https://github.com/mozilla-services/socorro/blob/main/webapp/crashstats/supersearch/form_fields.py
fn exact_match_default(value: String) -> String {
    const PREFIXES: &[&str] = &[
        // Negated operators (check longest first)
        "!__true__",
        "!__null__",
        "!$",
        "!~",
        "!^",
        "!@",
        "!=",
        "!",
        // Special tokens
        "__true__",
        "__null__",
        // Single-char operators
        "=",
        "~",
        "$",
        "^",
        "@",
        // Comparison operators (two-char before one-char)
        "<=",
        ">=",
        "<",
        ">",
    ];
    if PREFIXES.iter().any(|p| value.starts_with(p)) {
        value
    } else {
        format!("={}", value)
    }
}

pub struct SocorroClient {
    base_url: String,
    client: Client,
}

impl SocorroClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }

    fn get_auth_header(&self) -> Option<String> {
        auth::get_token()
    }

    /// Fetch the body of `/ProcessedCrash/` for `crash_id` as text.
    ///
    /// Validates the crash ID, sends the request (attaching the `Auth-Token`
    /// header only when `use_auth` is true and a token is available), and maps
    /// the response status onto the errors both `get_crash` and `get_crash_raw`
    /// report:
    ///
    ///   - `200` -> the body, as text;
    ///   - `404` -> [`Error::NotFound`] naming `crash_id`;
    ///   - `429` -> [`Error::RateLimited`];
    ///   - any other 4xx or 5xx -> [`Error::Http`], carrying reqwest's own
    ///     status and URL context;
    ///   - anything else, such as a `202` or an unfollowed redirect ->
    ///     [`Error::UnexpectedStatus`]. See [`status_error`] for why those last
    ///     two cannot share a representation.
    fn fetch_processed_crash_body(&self, crash_id: &str, use_auth: bool) -> Result<String> {
        if !crash_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(Error::InvalidCrashId(crash_id.to_string()));
        }

        let url = format!("{}/ProcessedCrash/", self.base_url);
        let mut request = self.client.get(&url).query(&[("crash_id", crash_id)]);

        if use_auth && let Some(token) = self.get_auth_header() {
            request = request.header("Auth-Token", token);
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => Ok(response.text()?),
            StatusCode::NOT_FOUND => Err(Error::NotFound(crash_id.to_string())),
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(status_error(response)),
        }
    }

    pub fn get_crash(&self, crash_id: &str, use_auth: bool) -> Result<ProcessedCrash> {
        let text = self.fetch_processed_crash_body(crash_id, use_auth)?;
        serde_json::from_str(&text).map_err(|e| {
            Error::ParseError(format!(
                "{}: {}",
                e,
                truncate_on_char_boundary(&text, PREVIEW_BYTES)
            ))
        })
    }

    /// Fetch `/ProcessedCrash/` for `crash_id` as an untyped `serde_json::Value`.
    ///
    /// Unlike `get_crash`, which deserializes into `ProcessedCrash` and thereby
    /// keeps only the fields that struct declares, this preserves every key the
    /// server sent. The body is still parsed (not echoed verbatim), so a
    /// malformed response yields `Error::ParseError` exactly as `get_crash` does.
    ///
    /// Response statuses are mapped exactly as for `get_crash`; see
    /// `fetch_processed_crash_body`, which both share, for the full table.
    ///
    /// `use_auth` is honoured identically to `get_crash`: the `Auth-Token`
    /// header is attached only when it is true and a token is available. Callers
    /// that will emit JSON must pass `false` so the server strips protected
    /// fields (registers, `mac_boot_args`, …) from `json_dump` server-side.
    pub fn get_crash_raw(&self, crash_id: &str, use_auth: bool) -> Result<serde_json::Value> {
        let text = self.fetch_processed_crash_body(crash_id, use_auth)?;
        serde_json::from_str(&text).map_err(|e| {
            Error::ParseError(format!(
                "{}: {}",
                e,
                truncate_on_char_boundary(&text, PREVIEW_BYTES)
            ))
        })
    }

    pub fn get_bugs(&self, signatures: &[String]) -> Result<BugsResponse> {
        let url = format!("{}/Bugs/", self.base_url);

        let mut request = self.client.get(&url);
        for sig in signatures {
            request = request.query(&[("signatures", sig)]);
        }

        if let Some(token) = self.get_auth_header() {
            request = request.header("Auth-Token", token);
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text()?;
                serde_json::from_str(&text).map_err(|e| {
                    Error::ParseError(format!(
                        "{}: {}",
                        e,
                        truncate_on_char_boundary(&text, PREVIEW_BYTES)
                    ))
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(status_error(response)),
        }
    }

    pub fn get_signatures_by_bugs(&self, bug_ids: &[u64]) -> Result<BugsResponse> {
        let url = format!("{}/SignaturesByBugs/", self.base_url);

        let mut request = self.client.get(&url);
        for id in bug_ids {
            request = request.query(&[("bug_ids", id.to_string())]);
        }

        if let Some(token) = self.get_auth_header() {
            request = request.header("Auth-Token", token);
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text()?;
                serde_json::from_str(&text).map_err(|e| {
                    Error::ParseError(format!(
                        "{}: {}",
                        e,
                        truncate_on_char_boundary(&text, PREVIEW_BYTES)
                    ))
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(status_error(response)),
        }
    }

    pub fn search(&self, params: SearchParams) -> Result<SearchResponse> {
        let url = format!("{}/SuperSearch/", self.base_url);

        let mut query_params = vec![
            ("product", params.product),
            ("_results_number", params.limit.to_string()),
            ("_sort", params.sort),
        ];

        for col in [
            "uuid",
            "date",
            "signature",
            "product",
            "version",
            "platform",
            "build_id",
            "release_channel",
            "platform_version",
        ] {
            query_params.push(("_columns", col.to_string()));
        }

        query_params.push(("date", format!(">={}", params.date_from)));
        if let Some(ref to) = params.date_to {
            let end = chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d").unwrap()
                + chrono::Duration::days(1);
            query_params.push(("date", format!("<{}", end.format("%Y-%m-%d"))));
        }

        if let Some(sig) = params.signature {
            push_filter(&mut query_params, "signature", sig);
        }

        if let Some(proto_sig) = params.proto_signature {
            push_filter(&mut query_params, "proto_signature", proto_sig);
        }

        if let Some(ver) = params.version {
            push_filter(&mut query_params, "version", ver);
        }

        if let Some(plat) = params.platform {
            push_filter(&mut query_params, "platform", plat);
        }

        if let Some(arch) = params.cpu_arch {
            push_filter(&mut query_params, "cpu_arch", arch);
        }

        if let Some(channel) = params.release_channel {
            push_filter(&mut query_params, "release_channel", channel);
        }

        if let Some(platform_version) = params.platform_version {
            push_filter(&mut query_params, "platform_version", platform_version);
        }

        if let Some(process_type) = params.process_type {
            push_filter(&mut query_params, "process_type", process_type);
        }

        for facet in params.facets {
            query_params.push(("_facets", facet));
        }

        if let Some(size) = params.facets_size {
            query_params.push(("_facets_size", size.to_string()));
        }

        let mut request = self.client.get(&url);
        for (key, value) in query_params {
            request = request.query(&[(key, value)]);
        }

        if let Some(token) = self.get_auth_header() {
            request = request.header("Auth-Token", token);
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text()?;
                serde_json::from_str(&text).map_err(|e| {
                    Error::ParseError(format!(
                        "{}: {}",
                        e,
                        truncate_on_char_boundary(&text, PREVIEW_BYTES)
                    ))
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(status_error(response)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::TestServer;
    use serial_test::serial;

    fn test_client() -> SocorroClient {
        SocorroClient::new("https://crash-stats.mozilla.org/api".to_string())
    }

    /// A syntactically valid crash ID. Only its characters matter: the test
    /// server answers whatever is asked for, so no live crash is involved and
    /// this cannot rot when Socorro expires the report.
    const CRASH_ID: &str = "b98bbb81-3ff6-4825-991f-6a0b30260901";

    /// A client whose `base_url` is the loopback test server.
    fn client_for(server: &TestServer) -> SocorroClient {
        SocorroClient::new(server.base_url())
    }

    /// A body that is not JSON and whose byte 200 falls *inside* a character:
    /// 199 ASCII bytes followed by a three-byte em dash occupying bytes
    /// 199..202. This is the exact shape that makes a preview built by
    /// slicing the body at a fixed byte cap panic.
    fn body_split_mid_character() -> String {
        let body = "a".repeat(199) + "\u{2014}";
        assert_eq!(body.len(), 202);
        assert!(!body.is_char_boundary(200));
        body
    }

    /// Assert that `error` is a `ParseError` whose preview was cut back to the
    /// character boundary before the cap: 199 `a`s, and no em dash.
    fn assert_preview_truncated_at_the_boundary(error: Error) {
        match error {
            Error::ParseError(message) => {
                assert!(
                    message.ends_with(&"a".repeat(199)),
                    "preview did not end at the character boundary: {message}"
                );
                assert!(
                    !message.contains('\u{2014}'),
                    "preview included the character the cap fell inside: {message}"
                );
            }
            other => panic!("expected Error::ParseError, got {other:?}"),
        }
    }

    /// The smallest `SearchParams` the client will accept. `date_to` is left
    /// `None` because `search` parses it with `unwrap`.
    fn minimal_search_params() -> SearchParams {
        SearchParams {
            signature: None,
            proto_signature: None,
            product: "Firefox".to_string(),
            version: None,
            platform: None,
            cpu_arch: None,
            release_channel: None,
            platform_version: None,
            process_type: None,
            date_from: "2026-09-01".to_string(),
            date_to: None,
            limit: 10,
            facets: vec![],
            facets_size: None,
            sort: "-date".to_string(),
        }
    }

    /// Run `body` with `SOCORRO_API_TOKEN_PATH` pointing at a throwaway token
    /// file, so that `auth::get_token` is guaranteed to find *a* token even on
    /// a machine with no keychain entry.
    ///
    /// That guarantee is what makes the "no `Auth-Token` header" assertions
    /// load-bearing. Without it, a machine holding no credential at all would
    /// satisfy them vacuously: the client would send no header because it had
    /// none to send, not because it honoured `use_auth`, and the test would
    /// keep passing even if `use_auth` were ignored outright.
    ///
    /// The file written here holds an obvious placeholder, never a real
    /// credential, and the real file named by the variable is never read. The
    /// test server records header *names* only (see
    /// [`crate::test_server::RecordedRequest`]), so no token value -- not even
    /// this placeholder -- can reach an assertion message or a backtrace.
    fn with_a_token_available<T>(body: impl FnOnce() -> T) -> T {
        let dir = tempfile::tempdir().expect("create a temp dir for the token file");
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "placeholder-not-a-real-token")
            .expect("write the throwaway token file");

        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::set_var("SOCORRO_API_TOKEN_PATH", &token_path) };
        // The keychain is consulted before the file, so the token the client
        // finds may come from either source; all that matters is that one
        // exists. Assert it, or the negative assertions prove nothing.
        assert!(
            auth::get_token().is_some(),
            "no API token is reachable, so the no-token assertions would pass vacuously"
        );
        let result = body();
        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::remove_var("SOCORRO_API_TOKEN_PATH") };
        result
    }

    /// Whether the one request `server` answered carried an `Auth-Token`
    /// header. Asserts the request was recorded at all, so an absent header is
    /// a real observation rather than an empty log.
    fn sent_an_auth_token(server: &TestServer) -> bool {
        let requests = server.requests();
        assert_eq!(requests.len(), 1, "expected exactly one request");
        assert!(
            requests[0].has_header("host"),
            "the request head was not recorded, so nothing can be concluded from it"
        );
        requests[0].has_header("auth-token")
    }

    // `CLAUDE.md` makes it a security invariant that crash output destined for
    // JSON is fetched *without* the API token: with no token the server strips
    // the protected fields (registers, `mac_boot_args`, ...) out of `json_dump`
    // itself, which is the only thing keeping them out of the raw `--full`
    // passthrough. `should_use_auth` in `src/commands/crash.rs` decides this,
    // and is tested there -- but nothing tested the plumbing that consumes its
    // answer, so making `fetch_processed_crash_body` ignore `use_auth` and
    // always attach the token left the whole suite green. The four tests below
    // close that gap at the wire.

    #[test]
    #[serial]
    fn get_crash_sends_no_auth_token_when_use_auth_is_false() {
        with_a_token_available(|| {
            let server = TestServer::start();
            server.push_response(200, "{}");

            let _ = client_for(&server).get_crash(CRASH_ID, false);

            assert!(
                !sent_an_auth_token(&server),
                "get_crash sent the API token despite use_auth = false"
            );
        });
    }

    #[test]
    #[serial]
    fn get_crash_raw_sends_no_auth_token_when_use_auth_is_false() {
        with_a_token_available(|| {
            let server = TestServer::start();
            server.push_response(200, "{}");

            let _ = client_for(&server).get_crash_raw(CRASH_ID, false);

            assert!(
                !sent_an_auth_token(&server),
                "get_crash_raw sent the API token despite use_auth = false"
            );
        });
    }

    #[test]
    #[serial]
    fn get_crash_sends_the_auth_token_when_use_auth_is_true() {
        with_a_token_available(|| {
            let server = TestServer::start();
            server.push_response(200, "{}");

            let _ = client_for(&server).get_crash(CRASH_ID, true);

            assert!(
                sent_an_auth_token(&server),
                "get_crash did not send the API token despite use_auth = true"
            );
        });
    }

    #[test]
    #[serial]
    fn get_crash_raw_sends_the_auth_token_when_use_auth_is_true() {
        with_a_token_available(|| {
            let server = TestServer::start();
            server.push_response(200, "{}");

            let _ = client_for(&server).get_crash_raw(CRASH_ID, true);

            assert!(
                sent_an_auth_token(&server),
                "get_crash_raw did not send the API token despite use_auth = true"
            );
        });
    }

    /// Assert that `error` is an `UnexpectedStatus` naming `expected_status`
    /// and a URL mentioning `expected_path`.
    fn assert_unexpected_status(error: Error, expected_status: u16, expected_path: &str) {
        match error {
            Error::UnexpectedStatus { status, url } => {
                assert_eq!(status, expected_status);
                assert!(
                    url.contains(expected_path),
                    "url {url} does not name the endpoint {expected_path}"
                );
            }
            other => panic!("expected Error::UnexpectedStatus, got {other:?}"),
        }
    }

    /// Assert that `error` is a reqwest-backed `Http` error carrying
    /// `expected_status`, i.e. that a genuine 4xx/5xx was *not* rerouted to
    /// `UnexpectedStatus` and so kept reqwest's richer context.
    fn assert_http_error_with_status(error: Error, expected_status: u16) {
        match error {
            Error::Http(err) => assert_eq!(
                err.status().map(|status| status.as_u16()),
                Some(expected_status),
                "Http error did not carry the response status: {err}"
            ),
            other => panic!("expected Error::Http, got {other:?}"),
        }
    }

    #[test]
    fn get_crash_reports_an_unexpected_202_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("202 is not a body this client can use");

        assert_unexpected_status(error, 202, "/ProcessedCrash/");
    }

    #[test]
    fn get_crash_raw_reports_an_unexpected_202_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error = client_for(&server)
            .get_crash_raw(CRASH_ID, false)
            .expect_err("202 is not a body this client can use");

        assert_unexpected_status(error, 202, "/ProcessedCrash/");
    }

    #[test]
    fn get_bugs_reports_an_unexpected_202_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error = client_for(&server)
            .get_bugs(&["OOM | small".to_string()])
            .expect_err("202 is not a body this client can use");

        assert_unexpected_status(error, 202, "/Bugs/");
    }

    #[test]
    fn get_signatures_by_bugs_reports_an_unexpected_202_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error = client_for(&server)
            .get_signatures_by_bugs(&[1234567])
            .expect_err("202 is not a body this client can use");

        assert_unexpected_status(error, 202, "/SignaturesByBugs/");
    }

    #[test]
    fn search_reports_an_unexpected_202_without_panicking() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let error = client_for(&server)
            .search(minimal_search_params())
            .expect_err("202 is not a body this client can use");

        assert_unexpected_status(error, 202, "/SuperSearch/");
    }

    #[test]
    fn get_crash_reports_an_unexpected_301_without_panicking() {
        // A redirect the client is not configured to follow is the other
        // non-error status that can reach the fallthrough arm.
        let server = TestServer::start();
        server.push_response(301, "moved");

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("an unfollowed redirect yields no usable body");

        assert_unexpected_status(error, 301, "/ProcessedCrash/");
    }

    #[test]
    fn get_crash_maps_a_genuine_500_to_an_http_error() {
        let server = TestServer::start();
        server.push_response(500, "upstream exploded");

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("500 is a failure");

        assert_http_error_with_status(error, 500);
    }

    #[test]
    fn search_maps_a_genuine_500_to_an_http_error() {
        let server = TestServer::start();
        server.push_response(500, "upstream exploded");

        let error = client_for(&server)
            .search(minimal_search_params())
            .expect_err("500 is a failure");

        assert_http_error_with_status(error, 500);
    }

    #[test]
    fn get_bugs_maps_a_genuine_400_to_an_http_error() {
        let server = TestServer::start();
        server.push_response(400, "bad request");

        let error = client_for(&server)
            .get_bugs(&["OOM | small".to_string()])
            .expect_err("400 is a failure");

        assert_http_error_with_status(error, 400);
    }

    #[test]
    fn get_crash_maps_404_to_not_found() {
        let server = TestServer::start();
        server.push_response(404, "not found");

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("404 is a failure");

        match error {
            Error::NotFound(id) => assert_eq!(id, CRASH_ID),
            other => panic!("expected Error::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn get_crash_raw_maps_404_to_not_found() {
        let server = TestServer::start();
        server.push_response(404, "not found");

        let error = client_for(&server)
            .get_crash_raw(CRASH_ID, false)
            .expect_err("404 is a failure");

        match error {
            Error::NotFound(id) => assert_eq!(id, CRASH_ID),
            other => panic!("expected Error::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn get_crash_maps_429_to_rate_limited() {
        let server = TestServer::start();
        server.push_response(429, "slow down");

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("429 is a failure");

        assert!(matches!(error, Error::RateLimited), "got {error:?}");
    }

    #[test]
    fn get_crash_raw_maps_429_to_rate_limited() {
        let server = TestServer::start();
        server.push_response(429, "slow down");

        let error = client_for(&server)
            .get_crash_raw(CRASH_ID, false)
            .expect_err("429 is a failure");

        assert!(matches!(error, Error::RateLimited), "got {error:?}");
    }

    #[test]
    fn get_bugs_maps_429_to_rate_limited() {
        let server = TestServer::start();
        server.push_response(429, "slow down");

        let error = client_for(&server)
            .get_bugs(&["OOM | small".to_string()])
            .expect_err("429 is a failure");

        assert!(matches!(error, Error::RateLimited), "got {error:?}");
    }

    #[test]
    fn get_signatures_by_bugs_maps_429_to_rate_limited() {
        let server = TestServer::start();
        server.push_response(429, "slow down");

        let error = client_for(&server)
            .get_signatures_by_bugs(&[1234567])
            .expect_err("429 is a failure");

        assert!(matches!(error, Error::RateLimited), "got {error:?}");
    }

    #[test]
    fn search_maps_429_to_rate_limited() {
        let server = TestServer::start();
        server.push_response(429, "slow down");

        let error = client_for(&server)
            .search(minimal_search_params())
            .expect_err("429 is a failure");

        assert!(matches!(error, Error::RateLimited), "got {error:?}");
    }

    #[test]
    fn get_crash_raw_returns_the_body_on_200() {
        // Guards the error-mapping tests above against passing vacuously: the
        // OK arm must still hand back a parsed body.
        let server = TestServer::start();
        server.push_response(200, r#"{"uuid": "abc", "signature": "OOM | small"}"#);

        let value = client_for(&server)
            .get_crash_raw(CRASH_ID, false)
            .expect("a valid JSON body on 200 must deserialize");

        assert_eq!(value["signature"], "OOM | small");
    }

    #[test]
    fn get_crash_previews_a_body_split_mid_character_without_panicking() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_character());

        let error = client_for(&server)
            .get_crash(CRASH_ID, false)
            .expect_err("a non-JSON body must not deserialize");

        assert_preview_truncated_at_the_boundary(error);
    }

    #[test]
    fn get_crash_raw_previews_a_body_split_mid_character_without_panicking() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_character());

        let error = client_for(&server)
            .get_crash_raw(CRASH_ID, false)
            .expect_err("a non-JSON body must not deserialize");

        assert_preview_truncated_at_the_boundary(error);
    }

    #[test]
    fn get_bugs_previews_a_body_split_mid_character_without_panicking() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_character());

        let error = client_for(&server)
            .get_bugs(&["OOM | small".to_string()])
            .expect_err("a non-JSON body must not deserialize");

        assert_preview_truncated_at_the_boundary(error);
    }

    #[test]
    fn get_signatures_by_bugs_previews_a_body_split_mid_character_without_panicking() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_character());

        let error = client_for(&server)
            .get_signatures_by_bugs(&[1234567])
            .expect_err("a non-JSON body must not deserialize");

        assert_preview_truncated_at_the_boundary(error);
    }

    #[test]
    fn search_previews_a_body_split_mid_character_without_panicking() {
        let server = TestServer::start();
        server.push_response(200, body_split_mid_character());

        let error = client_for(&server)
            .search(minimal_search_params())
            .expect_err("a non-JSON body must not deserialize");

        assert_preview_truncated_at_the_boundary(error);
    }

    #[test]
    fn test_exact_match_default_plain_value() {
        assert_eq!(
            exact_match_default("OOM | small".to_string()),
            "=OOM | small"
        );
    }

    #[test]
    fn test_exact_match_default_contains_prefix() {
        assert_eq!(
            exact_match_default("~AudioDecoder".to_string()),
            "~AudioDecoder"
        );
    }

    #[test]
    fn test_exact_match_default_exact_prefix() {
        assert_eq!(
            exact_match_default("=OOM | small".to_string()),
            "=OOM | small"
        );
    }

    #[test]
    fn test_exact_match_default_starts_with_prefix() {
        assert_eq!(exact_match_default("$OOM".to_string()), "$OOM");
    }

    #[test]
    fn test_exact_match_default_not_prefix() {
        assert_eq!(
            exact_match_default("!OOM | small".to_string()),
            "!OOM | small"
        );
    }

    #[test]
    fn test_exact_match_default_negated_contains_prefix() {
        assert_eq!(
            exact_match_default("!~AudioDecoder".to_string()),
            "!~AudioDecoder"
        );
    }

    #[test]
    fn test_exact_match_default_regex_prefix() {
        assert_eq!(
            exact_match_default("@OOM.*small".to_string()),
            "@OOM.*small"
        );
    }

    #[test]
    fn test_exact_match_default_greater_than_prefix() {
        assert_eq!(exact_match_default(">10.0".to_string()), ">10.0");
    }

    #[test]
    fn test_exact_match_default_greater_equal_prefix() {
        assert_eq!(exact_match_default(">=120.0".to_string()), ">=120.0");
    }

    #[test]
    fn test_exact_match_default_null_token() {
        assert_eq!(exact_match_default("__null__".to_string()), "__null__");
    }

    #[test]
    fn test_push_filter_string_field_gets_exact_prefix() {
        let mut params = vec![];
        push_filter(&mut params, "signature", "OOM | small".to_string());
        assert_eq!(params[0], ("signature", "=OOM | small".to_string()));
    }

    #[test]
    fn test_push_filter_string_field_preserves_operator() {
        let mut params = vec![];
        push_filter(&mut params, "signature", "~AudioDecoder".to_string());
        assert_eq!(params[0], ("signature", "~AudioDecoder".to_string()));
    }

    #[test]
    fn test_push_filter_enum_field_no_prefix() {
        let mut params = vec![];
        push_filter(&mut params, "release_channel", "nightly".to_string());
        assert_eq!(params[0], ("release_channel", "nightly".to_string()));
    }

    #[test]
    fn test_invalid_crash_id_with_spaces() {
        let client = test_client();
        let result = client.get_crash("invalid crash id", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_invalid_crash_id_with_special_chars() {
        let client = test_client();
        let result = client.get_crash("abc123!@#$", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_invalid_crash_id_with_semicolon() {
        // This could be an injection attempt
        let client = test_client();
        let result = client.get_crash("abc123; DROP TABLE crashes;", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_raw_invalid_crash_id_with_spaces() {
        let client = test_client();
        let result = client.get_crash_raw("invalid crash id", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_raw_invalid_crash_id_with_special_chars() {
        let client = test_client();
        let result = client.get_crash_raw("abc123!@#$", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_raw_invalid_crash_id_with_semicolon() {
        // This could be an injection attempt
        let client = test_client();
        let result = client.get_crash_raw("abc123; DROP TABLE crashes;", true);
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_valid_crash_id_format() {
        // Valid UUIDs should contain only hex chars and dashes
        let crash_id = "247653e8-7a18-4836-97d1-42a720260120";
        // We can't test the full request without mocking, but we can verify
        // the validation passes by checking the ID is considered valid syntactically
        assert!(crash_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn test_crash_id_validation_allows_hex_and_dashes() {
        // Test that the validation logic correctly allows valid characters
        let valid_id = "abcdef01-2345-6789-abcd-ef0123456789";
        assert!(valid_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));

        let invalid_id = "abcdef01-2345-6789-abcd-ef012345678g"; // 'g' is not hex
        assert!(
            !invalid_id
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-')
        );
    }
}
