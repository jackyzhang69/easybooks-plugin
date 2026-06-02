use crate::client::ApiClient;
use crate::config::{self, Config};
use crate::output;
use anyhow::Result;
use serde_json::json;

/// `easybooks login --token <eb_live_...> [--base-url <url>]`
///
/// Persists `{ api_key, base_url }` to ~/.easybooks/config.json (mode 0600).
/// The key is the user's personal EasyBooks API key; it both authenticates and
/// identifies the user, so there is no owner id to capture. The key is never
/// echoed — output reports the masked form only.
///
/// The persisted `base_url` is resolved through the documented precedence
/// (contract §6): `--base-url` arg → `$EASYBOOKS_API_URL` env → DEFAULT (PROD).
/// `base_url_arg` is `None` when `--base-url` was not passed (the clap default
/// is intentionally dropped) so the env tier is honoured before falling back to
/// PROD. The config-file tier is `None` here because login is *writing* the
/// file and must not read a stale value back.
pub fn login(token: &str, base_url_arg: Option<String>) -> Result<()> {
    let base_url = config::resolve_base_url(base_url_arg, None);
    config::save(token, &base_url)?;
    let path = config::config_path()?;
    output::print_json(&json!({
        "status": "ok",
        "path": path,
        "base_url": base_url,
        "api_key_masked": config::mask_key(token),
    }))
}

/// `easybooks whoami` → GET /api/integrations/whoami.
/// Reports the configured base_url + the user id and scope echoed by the backend
/// + the masked key.
pub fn whoami(client: &ApiClient, cfg: &Config) -> Result<()> {
    let backend = client.get("/api/integrations/whoami", vec![])?;
    let user_id = backend
        .get("user_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let scope = backend
        .get("scope")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    output::print_json(&json!({
        "base_url": cfg.base_url,
        "user_id": user_id,
        "scope": scope,
        "api_key_masked": cfg.api_key_masked(),
    }))
}
