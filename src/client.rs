// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::models::{ProcessedCrash, SearchParams, SearchResponse};
use crate::{auth, Error, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;

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

    pub fn get_crash(&self, crash_id: &str, use_auth: bool) -> Result<ProcessedCrash> {
        if !crash_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(Error::InvalidCrashId(crash_id.to_string()));
        }

        let url = format!("{}/ProcessedCrash/", self.base_url);
        let mut request = self.client.get(&url).query(&[("crash_id", crash_id)]);

        if use_auth {
            if let Some(token) = self.get_auth_header() {
                request = request.header("Auth-Token", token);
            }
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text()?;
                serde_json::from_str(&text).map_err(|e| {
                    Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)]))
                })
            }
            StatusCode::NOT_FOUND => Err(Error::NotFound(crash_id.to_string())),
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(Error::Http(response.error_for_status().unwrap_err())),
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

        let days_ago = chrono::Utc::now() - chrono::Duration::days(params.days as i64);
        query_params.push(("date", format!(">={}", days_ago.format("%Y-%m-%d"))));

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
                    Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)]))
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(Error::Http(response.error_for_status().unwrap_err())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> SocorroClient {
        SocorroClient::new("https://crash-stats.mozilla.org/api".to_string())
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
        assert!(!invalid_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-'));
    }
}
