use crate::client::ApiClient;
use crate::config;
use crate::identity::PLUGIN_IDENTITY;
use crate::output;
use anyhow::{bail, Result};
use jz_plugin_common::tell_jacky;
use jz_plugin_common::tell_jacky::{FeedbackDraft, FeedbackOutcome, FeedbackType};
use serde_json::json;

fn parse_kind(kind: &str) -> Result<FeedbackType> {
    match kind.trim() {
        "feature-request" => Ok(FeedbackType::FeatureRequest),
        "bug-report" => Ok(FeedbackType::BugReport),
        "knowledge-tip" => Ok(FeedbackType::KnowledgeTip),
        _ => bail!("kind must be feature-request, bug-report, or knowledge-tip"),
    }
}

/// `easybooks feedback create ...`
pub fn create(
    _client: &ApiClient,
    title: &str,
    description: &str,
    kind: &str,
    idempotency_key: &str,
    user_confirmed: bool,
) -> Result<()> {
    if !user_confirmed {
        bail!("refusing to send: pass --user-confirmed after the user approves the draft");
    }
    let title = title.trim();
    let description = description.trim();
    let idempotency_key = idempotency_key.trim();
    if title.is_empty() || description.is_empty() || idempotency_key.is_empty() {
        bail!("title, description, and idempotency_key are required");
    }
    let draft = FeedbackDraft {
        kind: parse_kind(kind)?,
        title: title.to_string(),
        description: description.to_string(),
        idempotency_key: idempotency_key.to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        url: None,
        context: Some(json!({
            "plugin_id": config::TELL_JACKY_PRODUCT,
            "source": "easybooks-cli",
        })),
    };
    let outcome = tell_jacky::submit(&PLUGIN_IDENTITY, &config::resolve_accountd_url(), draft)?;
    match outcome {
        FeedbackOutcome::Delivered { id } => output::print_json(&json!({
            "id": id,
            "delivery": "accountd",
            "status": "received",
            "ok": true,
        })),
        FeedbackOutcome::LocalMirrorOnly { reason } => {
            bail!("Tell Jacky not_delivered: feedback saved to local_mirror only — {reason}");
        }
    }
}

/// `easybooks feedback status --report-id ...`
pub fn status(client: &ApiClient, report_id: &str) -> Result<()> {
    let report_id = report_id.trim();
    if report_id.is_empty() {
        bail!("report_id is required");
    }
    let resp = client.tell_jacky_get(report_id)?;
    output::print_json(&resp)
}
