use anyhow::{Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::Value;

/// HTTP client for the EasyBooks backend integration endpoints (contract §3).
///
/// Every request carries `Authorization: Bearer <api_key>` — the user's personal
/// EasyBooks API key (`eb_live_...`). The key both authenticates and identifies
/// the user (its scope, `read` or `read_write`, gates reads vs writes), so there
/// is no separate owner-id header.
///
/// The api_key is NEVER logged or echoed; callers that need to surface it use
/// `Config::api_key_masked()` (`eb_***`).
pub struct ApiClient {
    base_url: String,
    api_key: String,
    client: Client,
}

impl ApiClient {
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        Self::new_with_timeout(base_url, api_key, std::time::Duration::from_secs(120))
    }

    pub fn new_with_timeout(
        base_url: String,
        api_key: String,
        timeout: std::time::Duration,
    ) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client: Client::builder()
                .timeout(timeout)
                .user_agent(concat!("easybooks-cli/", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    pub fn get(&self, path: &str, query: Vec<(&str, String)>) -> Result<Value> {
        let response = self.request("GET", path, query, None::<&Value>)?;
        parse_json_response(response)
    }

    pub fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<Value> {
        let response = self.request("POST", path, vec![], Some(body))?;
        parse_json_response(response)
    }

    fn request<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: Vec<(&str, String)>,
        body: Option<T>,
    ) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self
            .client
            .request(method.parse()?, url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(ACCEPT, "application/json");
        if !query.is_empty() {
            req = req.query(&query);
        }
        if let Some(body) = body {
            req = req.header(CONTENT_TYPE, "application/json").json(&body);
        }
        let response = req.send().context("request failed")?;
        ensure_success(response)
    }
}

fn ensure_success(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let text = response.text().unwrap_or_default();
    Err(anyhow::anyhow!("API error {}: {}", status.as_u16(), text))
}

fn parse_json_response(response: Response) -> Result<Value> {
    let text = response.text()?;
    if text.trim().is_empty() {
        return Ok(serde_json::json!({"success": true}));
    }
    Ok(serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw": text })))
}
