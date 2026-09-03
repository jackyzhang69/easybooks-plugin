use crate::config::{self, Config};
use crate::identity::PLUGIN_IDENTITY;
use anyhow::{bail, Context, Result};
use jz_plugin_common::auth::{self, AuthError};
use jz_plugin_common::http::{self, HttpError};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

/// HTTP client for EasyBooks backend integration endpoints.
///
/// Auth (exchange mode, aud=eb): durable `jz_` from the shared user slot is
/// exchanged via `jz-plugin-common`. Product integration routes receive the
/// in-memory JWT only; accountd product routes use the same crate path.
pub struct ApiClient {
    base_url: String,
    accountd_url: String,
    client: Client,
}

impl ApiClient {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        Self::new_with_timeout(cfg.base_url.clone(), Duration::from_secs(120))
    }

    #[allow(dead_code)]
    pub fn new(base_url: String) -> Result<Self> {
        Self::new_with_timeout(base_url, Duration::from_secs(120))
    }

    pub fn new_with_timeout(base_url: String, timeout: Duration) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            accountd_url: config::resolve_accountd_url(),
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

    pub fn delete(&self, path: &str) -> Result<Value> {
        let response = self.request("DELETE", path, vec![], None::<&Value>)?;
        parse_json_response(response)
    }

    pub fn send_with_body<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
    ) -> Result<Value> {
        let response = self.request(method, path, vec![], Some(body))?;
        parse_json_response(response)
    }

    /// GET /v1/products/easybooks/feedback/{id} on accountd using exchanged JWT.
    pub fn tell_jacky_get(&self, id: &str) -> Result<Value> {
        let url = format!(
            "{}/v1/products/{}/feedback/{}",
            self.accountd_url,
            config::TELL_JACKY_PRODUCT,
            id
        );
        accountd_get_json(&self.accountd_url, &url)
    }

    fn product_token(&self) -> Result<String> {
        auth::exchange(&PLUGIN_IDENTITY, &self.accountd_url)
            .map(|jwt| jwt.token)
            .map_err(map_auth_error)
    }

    fn request<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: Vec<(&str, String)>,
        body: Option<T>,
    ) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let send = |token: String| -> Result<Response> {
            let mut req = self
                .client
                .request(method.parse()?, &url)
                .header(ACCEPT, "application/json")
                .header(AUTHORIZATION, format!("Bearer {token}"));
            if !query.is_empty() {
                req = req.query(&query);
            }
            if let Some(body) = body.as_ref() {
                req = req.header(CONTENT_TYPE, "application/json").json(body);
            }
            req.send()
                .with_context(|| format!("{method} {path} failed"))
        };

        let response = send(self.product_token()?)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            auth::invalidate_exchange_cache(&PLUGIN_IDENTITY, &self.accountd_url)
                .map_err(map_auth_error)?;
            send(self.product_token()?)
        } else {
            Ok(response)
        }
    }
}

fn accountd_get_json(accountd_base: &str, url: &str) -> Result<Value> {
    let accountd_base = accountd_base.trim_end_matches('/');
    let mut auth_retry = false;
    loop {
        let jwt = auth::exchange(&PLUGIN_IDENTITY, accountd_base).map_err(map_auth_error)?;
        match http::send_json_value("GET", url, Some(&jwt.token), None) {
            Ok(response) => return Ok(response.body),
            Err(HttpError::Status { code: 401, .. }) if !auth_retry => {
                auth_retry = true;
                auth::invalidate_exchange_cache(&PLUGIN_IDENTITY, accountd_base)
                    .map_err(map_auth_error)?;
            }
            Err(HttpError::Status { code: 401, .. }) => {
                return Err(map_auth_error(AuthError::Unauthorized));
            }
            Err(HttpError::Status { code, body_excerpt }) => {
                bail!("Tell Jacky remote_error: {body_excerpt} (HTTP {code})");
            }
            Err(HttpError::Transport(message) | HttpError::Decode(message)) => {
                bail!("Tell Jacky request failed: {message}");
            }
        }
    }
}

fn map_auth_error(error: AuthError) -> anyhow::Error {
    match error {
        AuthError::NotConnected => anyhow::anyhow!(
            "not logged in: run 'easybooks login --token-stdin' with a portal owner token (jz_). Shared slot: ~/.jackyzhang.app/token/user.json"
        ),
        AuthError::Unauthorized => anyhow::anyhow!(
            "token rejected; re-run easybooks login --token-stdin with a valid portal owner token (jz_)"
        ),
        AuthError::WrongAudience => anyhow::anyhow!("exchanged JWT audience mismatch"),
        AuthError::Malformed => anyhow::anyhow!("exchanged JWT is malformed or missing required claims"),
        AuthError::Http(http_error) => anyhow::anyhow!("accountd request failed: {http_error}"),
    }
}

fn parse_json_response(response: Response) -> Result<Value> {
    let status = response.status();
    let body = response.text().context("reading response body")?;
    if body.trim().is_empty() {
        if status.is_success() {
            return Ok(json!({}));
        }
        bail!("HTTP {status} with empty body");
    }
    let value: Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "parsing JSON response (HTTP {status}): {}",
            truncate(&body, 200)
        )
    })?;
    if !status.is_success() {
        let message = value
            .get("error")
            .and_then(|v| v.as_str())
            .or_else(|| value.pointer("/error/message").and_then(|v| v.as_str()))
            .unwrap_or("request failed");
        bail!("HTTP {status}: {message}");
    }
    Ok(value)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
