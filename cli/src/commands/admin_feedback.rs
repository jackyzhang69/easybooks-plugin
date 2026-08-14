//! Admin Tell-Jacky feedback ops (plugin_id=easybooks).
//!
//! Auth chain: admin.json (durable jz_ admin) → exchange aud=portal + scope admin
//! → short-lived Portal JWT → accountd admin feedback routes.
//! Never uses user.json.

use crate::config;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::time::{Duration, Instant};

const PRODUCT: &str = "easybooks";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

struct AdminSession {
    jwt: String,
    accountd: String,
    http: reqwest::blocking::Client,
    expires_at: Instant,
}

fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("easybooks-cli/{CLIENT_VERSION}"))
        .build()?)
}

fn load_admin_session() -> Result<AdminSession> {
    let raw = config::read_admin_token()?.ok_or_else(|| {
        anyhow!(
            "admin not configured: install platform admin jz_ into {} (connect-easybooks-admin)",
            config::admin_token_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "~/.jackyzhang.app/token/admin.json".into())
        )
    })?;
    let accountd = config::resolve_accountd_url();
    let http = http_client()?;
    let (jwt, expires_in) = exchange_portal_admin(&http, &accountd, &raw)?;
    Ok(AdminSession {
        jwt,
        accountd,
        http,
        expires_at: Instant::now() + Duration::from_secs(expires_in.saturating_sub(30)),
    })
}

fn exchange_portal_admin(
    http: &reqwest::blocking::Client,
    accountd: &str,
    raw_admin: &str,
) -> Result<(String, u64)> {
    let url = format!("{accountd}/v1/token/exchange");
    let body = json!({
        "aud": "portal",
        "scopes": ["admin", "read", "write"],
    });
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {raw_admin}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("accountd token exchange failed")?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "admin exchange rejected (HTTP {}): need admin jz_ in admin.json with admin role. {}",
            status.as_u16(),
            truncate(&text, 240)
        );
    }
    let v: Value = serde_json::from_str(&text).context("invalid exchange JSON")?;
    let jwt = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("exchange response missing access_token"))?
        .to_string();
    let expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(300);
    Ok((jwt, expires_in))
}

impl AdminSession {
    fn bearer(&mut self) -> Result<String> {
        if Instant::now() >= self.expires_at {
            let raw = config::read_admin_token()?.ok_or_else(|| anyhow!("admin.json missing"))?;
            let (jwt, expires_in) = exchange_portal_admin(&self.http, &self.accountd, &raw)?;
            self.jwt = jwt;
            self.expires_at = Instant::now() + Duration::from_secs(expires_in.saturating_sub(30));
        }
        Ok(self.jwt.clone())
    }

    fn request(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, String)> {
        let token = self.bearer()?;
        let url = format!("{}{path}", self.accountd);
        let mut req = match method {
            "GET" => self.http.get(&url),
            "PATCH" => self.http.patch(&url),
            "POST" => self.http.post(&url),
            other => bail!("unsupported method {other}"),
        };
        req = req
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json");
        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").json(b);
        }
        let resp = req.send().with_context(|| format!("{method} {path} failed"))?;
        let status = resp.status().as_u16();
        let text = resp.text().unwrap_or_default();
        Ok((status, text))
    }
}

fn map_http(status: u16, text: &str) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 401 || status == 403 {
        bail!(
            "admin rejected (HTTP {status}): need admin.json jz_ that exchanges to portal admin JWT. {}",
            truncate(text, 240)
        );
    }
    if status == 400 {
        bail!(
            "admin bad request (HTTP 400): missing/invalid message or request_key. {}",
            truncate(text, 240)
        );
    }
    if status == 409 {
        bail!(
            "admin conflict (HTTP 409): illegal transition or request_key/message replay mismatch. {}",
            truncate(text, 240)
        );
    }
    bail!("HTTP {status}: {}", truncate(text, 240));
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn validate_id(id: &str) -> Result<String> {
    let id = id.trim();
    if id.is_empty() {
        bail!("--id required");
    }
    if id.contains('/') || id.chars().any(char::is_whitespace) {
        bail!("--id must be a single path segment (no slashes or whitespace)");
    }
    Ok(id.to_string())
}

fn print_list_human(text: &str) -> Result<()> {
    let v: Value = serde_json::from_str(text)?;
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        println!("(no feedback items)");
        return Ok(());
    }
    println!("# {} feedback item(s) [admin easybooks]", items.len());
    for it in &items {
        println!(
            "{}  [{}]  {}  — {}",
            it.get("id").and_then(|x| x.as_str()).unwrap_or("-"),
            it.get("status").and_then(|x| x.as_str()).unwrap_or("-"),
            it.get("type").and_then(|x| x.as_str()).unwrap_or("-"),
            it.get("title").and_then(|x| x.as_str()).unwrap_or("-"),
        );
    }
    Ok(())
}

pub fn list(status: Option<&str>, limit: usize, json_out: bool) -> Result<()> {
    let mut s = load_admin_session()?;
    let mut path = format!(
        "/v1/admin/products/{PRODUCT}/feedback?limit={}",
        limit.max(1)
    );
    if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
        path.push_str(&format!("&status={st}"));
    }
    let (code, text) = s.request("GET", &path, None)?;
    map_http(code, &text)?;
    if json_out {
        println!("{text}");
    } else {
        print_list_human(&text)?;
    }
    Ok(())
}

pub fn get(id: &str, json_out: bool) -> Result<()> {
    let id = validate_id(id)?;
    let mut s = load_admin_session()?;
    let path = format!("/v1/admin/products/{PRODUCT}/feedback/{id}");
    let (code, text) = s.request("GET", &path, None)?;
    map_http(code, &text)?;
    if json_out {
        println!("{text}");
    } else {
        let v: Value = serde_json::from_str(&text)?;
        println!(
            "id={} status={} type={} title={}",
            v.get("id").and_then(|x| x.as_str()).unwrap_or("-"),
            v.get("status").and_then(|x| x.as_str()).unwrap_or("-"),
            v.get("type").and_then(|x| x.as_str()).unwrap_or("-"),
            v.get("title").and_then(|x| x.as_str()).unwrap_or("-"),
        );
        if let Some(d) = v.get("description").and_then(|x| x.as_str()) {
            println!("description:\n{d}");
        }
        if let Some(updates) = v.get("updates").and_then(|x| x.as_array()) {
            for update in updates {
                let kind = update
                    .get("event_type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("reply");
                let message = update.get("message").and_then(|x| x.as_str()).unwrap_or("");
                println!("reply [{kind}]: {message}");
            }
        }
    }
    Ok(())
}

fn patch(id: &str, body: Value, json_out: bool) -> Result<()> {
    let id = validate_id(id)?;
    let mut s = load_admin_session()?;
    let path = format!("/v1/admin/products/{PRODUCT}/feedback/{id}");
    let (code, text) = s.request("PATCH", &path, Some(&body))?;
    map_http(code, &text)?;
    if json_out {
        println!("{text}");
    } else {
        println!("ok");
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(st) = v.get("status").and_then(|x| x.as_str()) {
                println!("status={st}");
            }
        }
    }
    Ok(())
}

pub fn triage(id: &str, message: &str, request_key: &str, json_out: bool) -> Result<()> {
    let message = message.trim();
    let request_key = request_key.trim();
    if message.is_empty() || request_key.is_empty() {
        bail!("triage requires --message and --request-key");
    }
    patch(
        id,
        json!({
            "status": "triaged",
            "message": message,
            "request_key": request_key,
        }),
        json_out,
    )
}

pub fn close(id: &str, message: &str, request_key: &str, json_out: bool) -> Result<()> {
    let message = message.trim();
    let request_key = request_key.trim();
    if message.is_empty() || request_key.is_empty() {
        bail!("close requires --message and --request-key");
    }
    patch(
        id,
        json!({
            "status": "closed",
            "message": message,
            "request_key": request_key,
        }),
        json_out,
    )
}

pub fn direct_close(
    id: &str,
    ack_message: &str,
    resolution_message: &str,
    request_key: &str,
    json_out: bool,
) -> Result<()> {
    let ack = ack_message.trim();
    let res = resolution_message.trim();
    let request_key = request_key.trim();
    if ack.is_empty() || res.is_empty() || request_key.is_empty() {
        bail!("direct-close requires --ack-message, --resolution-message, and --request-key");
    }
    patch(
        id,
        json!({
            "status": "closed",
            "acknowledgement_message": ack,
            "resolution_message": res,
            "request_key": request_key,
        }),
        json_out,
    )
}

/// Install admin token from stdin into admin.json (operator connect).
pub fn login_admin_from_stdin() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read admin token from stdin")?;
    let token = buf.trim();
    if token.is_empty() {
        bail!("stdin empty: pipe one admin jz_ line");
    }
    if !token.starts_with("jz_") || token.chars().any(char::is_whitespace) {
        bail!("admin token must be a single jz_ value");
    }
    config::write_admin_token(token)?;
    // Verify exchange works
    let _ = load_admin_session()?;
    println!(
        "{}",
        serde_json::json!({
            "status": "ok",
            "slot": "admin",
            "path": config::admin_token_path()?.display().to_string(),
        })
    );
    Ok(())
}
