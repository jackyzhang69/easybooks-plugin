use crate::config::{self, AuthKind, Config, ACCOUNTD_AUDIENCE};
use anyhow::{bail, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// HTTP client for EasyBooks backend integration endpoints + portal Tell-Jacky.
///
/// Auth: platform owner `jz_...` from shared `token/user.json`.
/// Product calls send the owner token (or an in-memory exchanged aud=eb JWT when needed).
pub struct ApiClient {
    base_url: String,
    accountd_url: String,
    credential: String,
    auth_kind: AuthKind,
    client: Client,
    exchanged: Mutex<Option<ExchangedToken>>,
}

struct ExchangedToken {
    access_token: String,
    expires_at: Instant,
}

impl ApiClient {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        Self::new_with_timeout(
            cfg.base_url.clone(),
            cfg.credential.clone(),
            cfg.auth_kind,
            Duration::from_secs(120),
        )
    }

    #[allow(dead_code)]
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        if !api_key.starts_with("jz_") {
            bail!("EasyBooks client requires a platform jz_ credential");
        }
        Self::new_with_timeout(base_url, api_key, AuthKind::PortalOwner, Duration::from_secs(120))
    }

    pub fn new_with_timeout(
        base_url: String,
        credential: String,
        auth_kind: AuthKind,
        timeout: Duration,
    ) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            accountd_url: config::resolve_accountd_url(),
            credential,
            auth_kind,
            client: Client::builder()
                .timeout(timeout)
                .user_agent(concat!("easybooks-cli/", env!("CARGO_PKG_VERSION")))
                .build()?,
            exchanged: Mutex::new(None),
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

    pub fn send_with_body<T: Serialize>(&self, method: &str, path: &str, body: &T) -> Result<Value> {
        let response = self.request(method, path, vec![], Some(body))?;
        parse_json_response(response)
    }

    /// POST /v1/products/easybooks/feedback on accountd using exchanged JWT.
    pub fn tell_jacky_create(&self, body: &Value) -> Result<Value> {
        if self.auth_kind != AuthKind::PortalOwner {
            bail!(
                "Tell Jacky requires a portal owner token (jz_); run easybooks login --token-stdin with the portal token"
            );
        }
        // Owner jz_ is accepted directly on Tell-Jacky product routes.
        let token = self.credential.clone();
        let url = format!(
            "{}/v1/products/{}/feedback",
            self.accountd_url,
            config::TELL_JACKY_PRODUCT
        );
        let response = self
            .client
            .post(url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .json(body)
            .send()
            .context("Tell Jacky request failed")?;
        parse_accountd_response(response)
    }

    pub fn tell_jacky_get(&self, id: &str) -> Result<Value> {
        if self.auth_kind != AuthKind::PortalOwner {
            bail!(
                "Tell Jacky requires a portal owner token (jz_); run easybooks login --token-stdin with the portal token"
            );
        }
        let token = self.credential.clone();
        let url = format!(
            "{}/v1/products/{}/feedback/{}",
            self.accountd_url,
            config::TELL_JACKY_PRODUCT,
            id
        );
        let response = self
            .client
            .get(url)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .context("Tell Jacky status request failed")?;
        parse_accountd_response(response)
    }

    fn request<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: Vec<(&str, String)>,
        body: Option<T>,
    ) -> Result<Response> {
        let bearer = self.authorization_token()?;
        let url = format!("{}{}", self.base_url, path);
        let mut req = self
            .client
            .request(method.parse()?, &url)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {bearer}"));
        if !query.is_empty() {
            req = req.query(&query);
        }
        if let Some(body) = body {
            req = req.header(CONTENT_TYPE, "application/json").json(&body);
        }
        req.send().with_context(|| format!("{method} {path} failed"))
    }

    fn authorization_token(&self) -> Result<String> {
        match self.auth_kind {
            // Prefer owner jz_ end-to-end. Exchange remains available for
            // product APIs that still require an exact-audience app JWT.
            AuthKind::PortalOwner => Ok(self.credential.clone()),
        }
    }

    #[allow(dead_code)]
    fn exchange_owner_token(&self) -> Result<String> {
        {
            let guard = self.exchanged.lock().expect("exchange lock");
            if let Some(cached) = guard.as_ref() {
                if Instant::now() + Duration::from_secs(30) < cached.expires_at {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        let url = format!("{}/v1/token/exchange", self.accountd_url);
        let response = self
            .client
            .post(url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", self.credential))
            .json(&json!({ "aud": ACCOUNTD_AUDIENCE }))
            .send()
            .context("account service token exchange failed")?;
        let status = response.status();
        let body: Value = response
            .json()
            .context("account service returned non-JSON exchange response")?;
        if !status.is_success() {
            let code = body
                .pointer("/error/code")
                .and_then(|v| v.as_str())
                .unwrap_or("exchange_failed");
            let message = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("token exchange failed");
            bail!("accountd exchange {code}: {message} (HTTP {status})");
        }
        let access_token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .context("exchange response missing access_token")?
            .to_string();
        let expires_in = body
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(300)
            .clamp(30, 900);
        let mut guard = self.exchanged.lock().expect("exchange lock");
        *guard = Some(ExchangedToken {
            access_token: access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        });
        Ok(access_token)
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
        format!("parsing JSON response (HTTP {status}): {}", truncate(&body, 200))
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

fn parse_accountd_response(response: Response) -> Result<Value> {
    let status = response.status();
    let body = response.text().context("reading Tell Jacky response")?;
    let value: Value = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&body).with_context(|| {
            format!(
                "parsing Tell Jacky JSON (HTTP {status}): {}",
                truncate(&body, 200)
            )
        })?
    };
    if !status.is_success() {
        let code = value
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .unwrap_or("remote_error");
        let message = value
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("Tell Jacky failed");
        bail!("Tell Jacky {code}: {message} (HTTP {status})");
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
