//! Product Signals fail-open emitter for EasyBooks.
//!
//! Emits `posting_completed` to accountd after a successful write.
//! Never blocks or fails the user's bookkeeping command.

use crate::client::ApiClient;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(1);

const PLUGIN_ID: &str = "easybooks";
const EVENT_NAME: &str = "posting_completed";
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

fn disabled() -> bool {
    if std::env::var("EASYBOOKS_PORTAL_OFFLINE").map(|v| v == "1").unwrap_or(false) {
        return true;
    }
    if let Ok(v) = std::env::var("EASYBOOKS_PRODUCT_SIGNALS_ENABLED") {
        return v == "0" || v.eq_ignore_ascii_case("false");
    }
    false
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn event_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (now.as_secs() & 0xFFFFFFFF) as u32,
        (now.subsec_millis() & 0xFFFF) as u16,
        (seq & 0x0FFF) as u16,
        ((seq >> 12) & 0x0FFF) as u16,
        seq & 0xFFFFFFFFFFFF
    )
}

/// Fail-open after a bookkeeping write attempt.
pub fn emit_posting_completed(client: &ApiClient, outcome: &str, feature: &str, error_code: Option<&str>) {
    if disabled() {
        return;
    }
    emit_named(client, EVENT_NAME, "posting", outcome, feature, error_code);
}

/// Fail-open after invoice create.
pub fn emit_invoice_created(client: &ApiClient, outcome: &str, error_code: Option<&str>) {
    if disabled() {
        return;
    }
    emit_named(client, "invoice_created", "invoice", outcome, "invoice", error_code);
}

pub(crate) fn emit_named(client: &ApiClient, event_name: &str, key: &str, outcome: &str, feature: &str, error_code: Option<&str>) {
    if disabled() {
        return;
    }
    let now = now_rfc3339();
    let seq = COUNTER.load(Ordering::Relaxed);
    let mut event = json!({
        "event_id": event_id(),
        "event_name": event_name,
        "event_version": 1,
        "actor_class": "user",
        "source": "product_server",
        "environment": std::env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string()),
        "occurred_at": now,
        "idempotency_key": format!("easybooks:{key}:{}:{}", now, seq),
        "outcome": outcome,
        "feature_id": feature,
        "properties": { "outcome": outcome }
    });
    if let Some(code) = error_code {
        event["error_code"] = json!(code);
    }
    client.emit_signals_batch(PLUGIN_ID, json!({ "events": [event] }), DEFAULT_TIMEOUT);
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_epoch() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(1), (1970, 1, 2));
    }

    #[test]
    fn emit_disabled_is_noop() {
        std::env::set_var("EASYBOOKS_PRODUCT_SIGNALS_ENABLED", "0");
        // No client: disabled() returns before any HTTP. Constructing a dummy
        // client is unnecessary; the flag must short-circuit first.
        assert!(super::disabled());
    }
}
