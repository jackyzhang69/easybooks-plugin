use crate::client::ApiClient;
use crate::config;
use crate::output;
use anyhow::{bail, Result};
use serde_json::json;

/// `easybooks feedback create ...`
pub fn create(
    client: &ApiClient,
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
    let kind = match kind.trim() {
        "feature-request" | "bug-report" | "knowledge-tip" => kind.trim(),
        _ => bail!("kind must be feature-request, bug-report, or knowledge-tip"),
    };
    let body = json!({
        "product": config::TELL_JACKY_PRODUCT,
        "type": kind,
        "title": title,
        "description": description,
        "client_version": env!("CARGO_PKG_VERSION"),
        "idempotency_key": idempotency_key,
        "context": {
            "plugin_id": config::TELL_JACKY_PRODUCT,
            "source": "easybooks-cli",
        }
    });
    let resp = client.tell_jacky_create(&body)?;
    output::print_json(&resp)
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
