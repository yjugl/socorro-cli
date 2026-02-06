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

        let days_ago = chrono::Utc::now() - chrono::Duration::days(params.days as i64);
        query_params.push(("date", format!(">={}", days_ago.format("%Y-%m-%d"))));

        if let Some(sig) = params.signature {
            query_params.push(("signature", sig));
        }

        if let Some(ver) = params.version {
            query_params.push(("version", ver));
        }

        if let Some(plat) = params.platform {
            query_params.push(("os_name", plat));
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
