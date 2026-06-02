//! Invoice commands (contract §2 "Invoices").
//!   - `invoice create --json '<json>' [--dry-run]` → POST /api/integrations/ingest/invoice
//!   - `invoice send <invoice_id>`                  → POST /api/integrations/invoice/:id/send

use crate::client::ApiClient;
use crate::output;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};

/// `easybooks invoice create --json '<json>' [--dry-run]`
///
/// `<json>` = `{ "client": {...} | {"client_id":uuid}, "issue_date", "due_date",
/// "tax_rate?", "items":[{description,quantity,unit_price}], "notes?",
/// "payment_details?", "source_id?" }`. Subtotal/tax/total are computed
/// server-side; we only validate the envelope + inject the configured owner.
pub fn create(client: &ApiClient, raw: &str, dry_run: bool) -> Result<()> {
    let value: Value = serde_json::from_str(raw).context("--json is not valid JSON")?;
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("--json must be a JSON object"))?;

    // Minimal local validation — the backend is authoritative, but we catch
    // obvious mistakes before a round-trip.
    if obj.get("client").is_none() {
        return Err(anyhow!("--json missing `client` (object or {{client_id}})"));
    }
    if obj.get("issue_date").and_then(|v| v.as_str()).is_none() {
        return Err(anyhow!("--json missing string `issue_date` (YYYY-MM-DD)"));
    }
    if obj.get("due_date").and_then(|v| v.as_str()).is_none() {
        return Err(anyhow!("--json missing string `due_date` (YYYY-MM-DD)"));
    }
    match obj.get("items").and_then(|v| v.as_array()) {
        Some(items) if !items.is_empty() => {}
        _ => return Err(anyhow!("--json `items` must be a non-empty array")),
    }

    let body: Map<String, Value> = obj.clone();

    if dry_run {
        return output::print_json(&json!({
            "status": "dry_run",
            "would_post": "/api/integrations/ingest/invoice",
            "body": Value::Object(body),
        }));
    }

    let resp = client.post("/api/integrations/ingest/invoice", &Value::Object(body))?;
    output::print_json(&resp)
}

/// `easybooks invoice send <invoice_id>` → proxy to the integration send route
/// `POST /api/integrations/invoice/:id/send`. That route authenticates with the
/// integration key (which the client always sends) and resolves the owner user
/// id from the body/header/env, so the legacy `x-user-id` header is not needed.
/// Output is passed through unchanged.
pub fn send(client: &ApiClient, invoice_id: &str) -> Result<()> {
    let path = format!(
        "/api/integrations/invoice/{}/send",
        encode_segment(invoice_id)
    );
    let resp = client.post(&path, &json!({}))?;
    output::print_json(&resp)
}

fn encode_segment(value: &str) -> String {
    value.replace('/', "%2F").replace(' ', "%20")
}
