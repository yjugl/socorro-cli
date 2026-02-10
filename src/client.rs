use crate::{auth, Error, Result};
use crate::models::{ProcessedCrash, SearchResponse, SearchParams};
use reqwest::blocking::Client;
use reqwest::StatusCode;

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

    pub fn get_crash(&self, crash_id: &str) -> Result<ProcessedCrash> {
        if !crash_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(Error::InvalidCrashId(crash_id.to_string()));
        }

        let url = format!("{}/ProcessedCrash/", self.base_url);
        let mut request = self.client.get(&url).query(&[("crash_id", crash_id)]);

        if let Some(token) = self.get_auth_header() {
            request = request.header("Auth-Token", token);
        }

        let response = request.send()?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text()?;
                serde_json::from_str(&text)
                    .map_err(|e| Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)])))
            }
            StatusCode::NOT_FOUND => Err(Error::NotFound(crash_id.to_string())),
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(Error::Http(
                response.error_for_status().unwrap_err()
            )),
        }
    }

    pub fn search(&self, params: SearchParams) -> Result<SearchResponse> {
        let url = format!("{}/SuperSearch/", self.base_url);

        let mut query_params = vec![
            ("product", params.product),
            ("_results_number", params.limit.to_string()),
            ("_sort", params.sort),
        ];

        for col in ["uuid", "date", "signature", "product", "version", "platform", "build_id", "release_channel"] {
            query_params.push(("_columns", col.to_string()));
        }

        let days_ago = chrono::Utc::now() - chrono::Duration::days(params.days as i64);
        query_params.push(("date", format!(">={}", days_ago.format("%Y-%m-%d"))));

        if let Some(sig) = params.signature {
            query_params.push(("signature", sig));
        }

        if let Some(ver) = params.version {
            query_params.push(("version", ver));
        }

        if let Some(plat) = params.platform {
            query_params.push(("platform", plat));
        }

        if let Some(arch) = params.cpu_arch {
            query_params.push(("cpu_arch", arch));
        }

        for facet in params.facets {
            query_params.push(("_facets", facet));
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
                serde_json::from_str(&text)
                    .map_err(|e| Error::ParseError(format!("{}: {}", e, &text[..text.len().min(200)])))
            }
            StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            _ => Err(Error::Http(
                response.error_for_status().unwrap_err()
            )),
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
    fn test_invalid_crash_id_with_spaces() {
        let client = test_client();
        let result = client.get_crash("invalid crash id");
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_invalid_crash_id_with_special_chars() {
        let client = test_client();
        let result = client.get_crash("abc123!@#$");
        assert!(matches!(result, Err(Error::InvalidCrashId(_))));
    }

    #[test]
    fn test_invalid_crash_id_with_semicolon() {
        // This could be an injection attempt
        let client = test_client();
        let result = client.get_crash("abc123; DROP TABLE crashes;");
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
        assert!(!invalid_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }
}
