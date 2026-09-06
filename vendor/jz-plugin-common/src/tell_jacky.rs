//! Tell-Jacky submit with mandatory local mirror.

use crate::auth::{self, AuthError};
use crate::home::{self, Home};
use crate::http::HttpError;
use crate::identity::PluginIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackType {
    #[serde(rename = "feature-request")]
    FeatureRequest,
    #[serde(rename = "bug-report")]
    BugReport,
    #[serde(rename = "knowledge-tip")]
    KnowledgeTip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackDraft {
    #[serde(rename = "type")]
    pub kind: FeedbackType,
    pub title: String,
    pub description: String,
    pub idempotency_key: String,
    pub client_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackOutcome {
    Delivered { id: String },
    LocalMirrorOnly { reason: String },
}

#[derive(Debug, Error)]
pub enum TellJackyError {
    #[error("{0}")]
    Auth(#[from] AuthError),
    #[error("{0}")]
    Home(#[from] io::Error),
    #[error("local feedback mirror failed: {0}")]
    Mirror(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MirrorEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    delivery: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MirrorStore {
    #[serde(default)]
    items: Vec<MirrorEntry>,
}

pub fn submit(
    identity: &PluginIdentity,
    accountd_base: &str,
    draft: FeedbackDraft,
) -> Result<FeedbackOutcome, TellJackyError> {
    let base = accountd_base.trim_end_matches('/');
    let path = format!("/v1/products/{}/feedback", identity.plugin_id);
    let url = format!("{base}{path}");
    let context = draft.context.clone().unwrap_or_else(|| {
        json!({
            "product": identity.plugin_id,
            "plugin_id": identity.plugin_id,
            "source": format!("{}-cli", identity.plugin_id),
        })
    });
    let mut body = json!({
        "type": serde_json::to_value(draft.kind).unwrap(),
        "title": draft.title,
        "description": draft.description,
        "idempotency_key": draft.idempotency_key,
        "client_version": draft.client_version,
        "context": context,
        "product": identity.plugin_id,
    });
    if let Some(url) = &draft.url {
        body["url"] = json!(url);
    }

    match auth::post_product_json(identity, accountd_base, &url, &body) {
        Ok(response) => {
            let id = response
                .body
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            if let Some(id) = id {
                mirror_local(identity, &draft, Some(&id), "accountd")?;
                Ok(FeedbackOutcome::Delivered { id })
            } else {
                let reason = "accountd response missing id".to_string();
                mirror_local(identity, &draft, None, "local_mirror")?;
                Ok(FeedbackOutcome::LocalMirrorOnly { reason })
            }
        }
        Err(AuthError::Unauthorized) => {
            mirror_local(identity, &draft, None, "local_mirror")?;
            Ok(FeedbackOutcome::LocalMirrorOnly {
                reason: "accountd rejected the credential".to_string(),
            })
        }
        Err(AuthError::Http(HttpError::Status { code, body_excerpt })) => {
            let reason = format!("HTTP {code}: {body_excerpt}");
            mirror_local(identity, &draft, None, "local_mirror")?;
            Ok(FeedbackOutcome::LocalMirrorOnly { reason })
        }
        Err(AuthError::Http(HttpError::Transport(message) | HttpError::Decode(message))) => {
            mirror_local(identity, &draft, None, "local_mirror")?;
            Ok(FeedbackOutcome::LocalMirrorOnly { reason: message })
        }
        Err(error) => {
            let reason = error.to_string();
            mirror_local(identity, &draft, None, "local_mirror")?;
            Ok(FeedbackOutcome::LocalMirrorOnly { reason })
        }
    }
}

fn mirror_local(
    identity: &PluginIdentity,
    draft: &FeedbackDraft,
    delivered_id: Option<&str>,
    delivery: &str,
) -> Result<(), TellJackyError> {
    let home = Home::resolve()?;
    home.ensure_plugin_runtime(identity.plugin_id)?;
    let path = home.feedback_mirror_path(identity.plugin_id);
    let mut store = load_mirror(&path).unwrap_or_default();
    let id = delivered_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("local-{}", draft.idempotency_key));
    store.items.push(MirrorEntry {
        id,
        kind: feedback_type_str(&draft.kind).to_string(),
        title: draft.title.clone(),
        description: draft.description.clone(),
        status: Some("received".to_string()),
        delivery: delivery.to_string(),
        created_at: Some(chrono::Utc::now().timestamp()),
    });
    let bytes = serde_json::to_vec_pretty(&store)
        .map_err(|error| TellJackyError::Mirror(error.to_string()))?;
    home::write_private_file(&path, &bytes).map_err(TellJackyError::Home)?;
    Ok(())
}

fn load_mirror(path: &std::path::Path) -> Result<MirrorStore, io::Error> {
    if !path.is_file() {
        return Ok(MirrorStore::default());
    }
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse feedback mirror: {error}"),
        )
    })
}

fn feedback_type_str(kind: &FeedbackType) -> &'static str {
    match kind {
        FeedbackType::FeatureRequest => "feature-request",
        FeedbackType::BugReport => "bug-report",
        FeedbackType::KnowledgeTip => "knowledge-tip",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AuthMode, PluginIdentity};
    use crate::test_env;

    const IDENTITY: PluginIdentity = PluginIdentity {
        plugin_id: "anydoc",
        aud: Some("anydoc"),
        auth_mode: AuthMode::Exchange,
        product_scopes: &["read", "write"],
    };

    fn draft() -> FeedbackDraft {
        FeedbackDraft {
            kind: FeedbackType::BugReport,
            title: "title".into(),
            description: "desc".into(),
            idempotency_key: "idem-1".into(),
            client_version: "0.1.0".into(),
            url: None,
            context: None,
        }
    }

    fn auth_test_jwt(aud: &str, issuer: &str) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","typ":"JWT"}"#);
        let exp = chrono::Utc::now().timestamp() + 120;
        let payload = format!(r#"{{"aud":"{aud}","iss":"{issuer}","exp":{exp}}}"#);
        let body = URL_SAFE_NO_PAD.encode(payload);
        format!("{header}.{body}.sig")
    }

    #[test]
    fn delivered_with_accountd_mirror() {
        test_env::with_home("jz_tell_jacky", |_tmp, mut server| {
            let issuer = server.url();
            let jwt = auth_test_jwt("anydoc", &issuer);
            server
                .mock("POST", "/v1/token/exchange")
                .with_status(200)
                .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
                .create();
            let feedback = server
                .mock("POST", "/v1/products/anydoc/feedback")
                .with_status(201)
                .with_body(r#"{"id":"fb_123","status":"received"}"#)
                .create();
            let outcome = submit(&IDENTITY, &issuer, draft()).unwrap();
            feedback.assert();
            assert_eq!(
                outcome,
                FeedbackOutcome::Delivered {
                    id: "fb_123".into()
                }
            );
            let home = Home::resolve().unwrap();
            let store: MirrorStore = serde_json::from_str(
                &fs::read_to_string(home.feedback_mirror_path("anydoc")).unwrap(),
            )
            .unwrap();
            assert_eq!(store.items.last().unwrap().delivery, "accountd");
        });
    }

    #[test]
    fn server_error_local_mirror_only() {
        test_env::with_home("jz_tell_jacky", |_tmp, mut server| {
            let issuer = server.url();
            let jwt = auth_test_jwt("anydoc", &issuer);
            server
                .mock("POST", "/v1/token/exchange")
                .with_status(200)
                .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
                .create();
            server
                .mock("POST", "/v1/products/anydoc/feedback")
                .with_status(503)
                .with_body("busy")
                .create();
            let outcome = submit(&IDENTITY, &issuer, draft()).unwrap();
            assert!(matches!(outcome, FeedbackOutcome::LocalMirrorOnly { .. }));
            let home = Home::resolve().unwrap();
            let store: MirrorStore = serde_json::from_str(
                &fs::read_to_string(home.feedback_mirror_path("anydoc")).unwrap(),
            )
            .unwrap();
            assert_eq!(store.items.last().unwrap().delivery, "local_mirror");
        });
    }

    #[test]
    fn network_down_local_mirror() {
        test_env::with_home("jz_tell_jacky", |_tmp, _server| {
            let outcome = submit(&IDENTITY, "http://127.0.0.1:1", draft()).unwrap();
            assert!(matches!(outcome, FeedbackOutcome::LocalMirrorOnly { .. }));
            assert!(!matches!(outcome, FeedbackOutcome::Delivered { .. }));
        });
    }
}
