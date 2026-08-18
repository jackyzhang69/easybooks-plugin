use crate::client::ApiClient;
use crate::config::{self, Config};
use crate::output;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::io::{self, IsTerminal, Read, Write};

/// `easybooks login --token-stdin [--base-url <url>]`
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
pub fn login_from_stdin(token_stdin: bool, base_url_arg: Option<String>) -> Result<()> {
    if !token_stdin {
        bail!("login requires --token-stdin");
    }
    let token = read_token()?;
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        bail!("token input must contain exactly one non-empty line");
    }
    let base_url = config::resolve_base_url(base_url_arg, None);
    let auth_kind = if token.starts_with("jz_") {
        "portal_owner"
    } else if token.starts_with("eb_") {
        "api_key"
    } else {
        bail!("token must be a portal owner jz_ token or a legacy eb_live_ API key");
    };
    config::save(&token, &base_url)?;
    let path = if auth_kind == "portal_owner" {
        "~/.jackyzhang.app/token/jz.json".to_string()
    } else {
        config::config_path()?
    };
    output::print_json(&json!({
        "status": "ok",
        "path": path,
        "base_url": base_url,
        "auth_kind": auth_kind,
        "api_key_masked": config::mask_key(&token),
    }))
}

fn read_token() -> Result<String> {
    let token = if io::stdin().is_terminal() {
        eprint!("EasyBooks API key: ");
        io::stderr().flush().context("showing API key prompt")?;
        rpassword::read_password().context("reading hidden API key from terminal")?
    } else {
        let mut bytes = Vec::new();
        io::stdin()
            .take(4097)
            .read_to_end(&mut bytes)
            .context("reading API key from standard input")?;
        if bytes.len() > 4096 {
            bail!("API key input exceeds the safety limit");
        }
        String::from_utf8(bytes).context("API key input must be UTF-8")?
    };
    let token = token
        .strip_suffix('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .unwrap_or(&token)
        .to_string();
    if token.len() > 4096 {
        bail!("API key input exceeds the safety limit");
    }
    Ok(token)
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
    let email = backend
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());
    let scope = backend
        .get("scope")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut payload = json!({
        "base_url": cfg.base_url,
        "user_id": user_id,
        "scope": scope,
        "api_key_masked": cfg.api_key_masked(),
    });
    if let Some(email) = email {
        payload
            .as_object_mut()
            .expect("whoami payload is an object")
            .insert("email".into(), json!(email));
    }
    output::print_json(&payload)
}
