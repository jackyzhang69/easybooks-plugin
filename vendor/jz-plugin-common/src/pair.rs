//! User-plane pair-session client (`contracts/pair-session-v1.md`).
//!
//! Admin HTTP stays in plugin-admin. This module never prints tokens.

use crate::auth::{self, AuthError};
use crate::identity::PluginIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PairError {
    #[error("{0}")]
    Auth(AuthError),
    #[error("pair session not found")]
    NotFound,
    #[error("pair session conflict")]
    Conflict,
    #[error("pair session expired")]
    Expired,
    #[error("{0}")]
    Rejected(String),
}

fn map_status(err: AuthError) -> PairError {
    match err {
        AuthError::Http(crate::http::HttpError::Status { code: 404, .. }) => PairError::NotFound,
        AuthError::Http(crate::http::HttpError::Status {
            code: 409,
            body_excerpt,
        }) => {
            if body_excerpt.contains("expired") {
                PairError::Expired
            } else {
                PairError::Conflict
            }
        }
        AuthError::Http(crate::http::HttpError::Status { body_excerpt, .. }) => {
            PairError::Rejected(body_excerpt)
        }
        other => PairError::Auth(other),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSession {
    pub id: String,
    pub product: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairMessage {
    pub id: String,
    pub session_id: String,
    pub from_role: String,
    pub kind: String,
    pub body: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

fn product_url(identity: &PluginIdentity, accountd_base: &str, suffix: &str) -> String {
    let base = accountd_base.trim_end_matches('/');
    format!("{base}/v1/products/{}{suffix}", identity.plugin_id)
}

pub fn join(
    identity: &PluginIdentity,
    accountd_base: &str,
    code: &str,
) -> Result<PairSession, PairError> {
    let url = product_url(identity, accountd_base, "/pair/join");
    let body = json!({ "code": code, "user_confirmed": true });
    match auth::post_product::<Value, PairSession>(identity, accountd_base, &url, &body) {
        Ok(resp) => Ok(resp.body),
        Err(err) => Err(map_status(err)),
    }
}

pub fn current(identity: &PluginIdentity, accountd_base: &str) -> Result<PairSession, PairError> {
    let url = product_url(identity, accountd_base, "/pair");
    match auth::get_product::<PairSession>(identity, accountd_base, &url) {
        Ok(resp) => Ok(resp.body),
        Err(err) => Err(map_status(err)),
    }
}

pub fn post(
    identity: &PluginIdentity,
    accountd_base: &str,
    session_id: &str,
    kind: &str,
    body: Value,
    idempotency_key: &str,
) -> Result<PairMessage, PairError> {
    let url = product_url(
        identity,
        accountd_base,
        &format!("/pair/{session_id}/messages"),
    );
    let payload = json!({
        "kind": kind,
        "body": body,
        "idempotency_key": idempotency_key,
    });
    match auth::post_product::<Value, PairMessage>(identity, accountd_base, &url, &payload) {
        Ok(resp) => Ok(resp.body),
        Err(err) => Err(map_status(err)),
    }
}

pub fn unread(
    identity: &PluginIdentity,
    accountd_base: &str,
    session_id: &str,
) -> Result<Vec<PairMessage>, PairError> {
    let url = product_url(
        identity,
        accountd_base,
        &format!("/pair/{session_id}/messages?unread=true&limit=20"),
    );
    match auth::get_product::<Items<PairMessage>>(identity, accountd_base, &url) {
        Ok(resp) => Ok(resp.body.items),
        Err(err) => Err(map_status(err)),
    }
}

pub fn mark_read(
    identity: &PluginIdentity,
    accountd_base: &str,
    session_id: &str,
    message_id: &str,
) -> Result<(), PairError> {
    let url = product_url(
        identity,
        accountd_base,
        &format!("/pair/{session_id}/messages/{message_id}/read"),
    );
    match auth::post_product::<Value, Value>(identity, accountd_base, &url, &json!({})) {
        Ok(_) => Ok(()),
        Err(err) => match err {
            AuthError::Http(crate::http::HttpError::Decode(_)) => Ok(()),
            other => Err(map_status(other)),
        },
    }
}

pub fn close(
    identity: &PluginIdentity,
    accountd_base: &str,
    session_id: &str,
    reason: &str,
) -> Result<PairSession, PairError> {
    let url = product_url(
        identity,
        accountd_base,
        &format!("/pair/{session_id}/close"),
    );
    let body = json!({ "reason": reason });
    match auth::post_product::<Value, PairSession>(identity, accountd_base, &url, &body) {
        Ok(resp) => Ok(resp.body),
        Err(err) => Err(map_status(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AuthMode, PluginIdentity};
    use crate::test_env;
    use serde_json::json;

    const IDENTITY: PluginIdentity = PluginIdentity {
        plugin_id: "anychat",
        aud: Some("anychat"),
        auth_mode: AuthMode::Exchange,
        product_scopes: &["read", "write"],
    };

    fn make_test_jwt(aud: &str, issuer: &str) -> String {
        use base64::Engine as _;
        use std::time::{SystemTime, UNIX_EPOCH};
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"ES256","typ":"JWT"}"#);
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            + 120.0;
        let payload = format!(r#"{{"aud":"{aud}","iss":"{issuer}","exp":{exp}}}"#);
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("{header}.{body}.signature")
    }

    #[test]
    fn join_posts_confirmed_code() {
        test_env::with_home("jz_test_pair", |_, mut server| {
            let issuer = server.url();
            let jwt = make_test_jwt("anychat", &issuer);
            let _ex = server
                .mock("POST", "/v1/token/exchange")
                .with_status(200)
                .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
                .create();
            let mock = server
                .mock("POST", "/v1/products/anychat/pair/join")
                .match_body(mockito::Matcher::PartialJson(json!({
                    "code": "ABC234",
                    "user_confirmed": true
                })))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"id":"sess1","product":"anychat","status":"open"}"#)
                .create();
            let sess = join(&IDENTITY, &issuer, "ABC234").expect("join");
            assert_eq!(sess.id, "sess1");
            mock.assert();
        });
    }
}
