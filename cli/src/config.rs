use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, io::Write, path::PathBuf};

/// Default backend: the PROD immicore-served eb-plugin, reached via the eb
/// frontend domain's nginx `/api` proxy (`https://easybooks.jackyzhang.app`
/// -> /api/ -> immicore Go eb-plugin). A fresh `easybooks login --token-stdin`
/// with no `--base-url` therefore targets production. Overrides still apply via
/// `--base-url` or `$EASYBOOKS_API_URL` (e.g. test
/// `https://easybooks-test.jackyzhang.app`, or LAN `http://192.168.1.69:8310`).
/// Production writes are governance-gated (contract §6): the connect/capabilities
/// skills warn before any production write.
pub const DEFAULT_BASE_URL: &str = "https://easybooks.jackyzhang.app";

/// Portal account service (accountd) origin for owner-token exchange + Tell-Jacky.
pub const DEFAULT_ACCOUNTD_URL: &str = "https://account.jackyzhang.app";

/// Exchange audience for EasyBooks product APIs (catalog_id easybooks ↔ aud eb).
pub const ACCOUNTD_AUDIENCE: &str = "eb";

/// Tell-Jacky path product id (not the exchange aud).
pub const TELL_JACKY_PRODUCT: &str = "easybooks";

/// On-disk shape of `~/.easybooks/config.json`.
///
/// - Legacy: `api_key` is `eb_live_...`.
/// - Portal: `api_key` may be empty/absent; owner credential lives once at
///   `~/.jackyzhang.app/token/jz.json`. Runtime data stays under `~/.easybooks/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub api_key: Option<String>,
    pub base_url: String,
}

/// Resolved runtime config.
#[derive(Debug, Clone)]
pub struct Config {
    /// Bearer secret after resolution: either `eb_live_...` or portal `jz_...`
    /// (exchange happens in the HTTP client, not here).
    pub credential: String,
    pub base_url: String,
    pub auth_kind: AuthKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// Legacy personal EasyBooks API key.
    ApiKey,
    /// Portal owner token (`jz_`); product calls must exchange aud=eb first.
    PortalOwner,
}

impl Config {
    /// Resolve effective config.
    ///
    /// Credential precedence:
    ///   1. `$EASYBOOKS_API_KEY` (legacy or jz_)
    ///   2. `~/.easybooks/config.json` api_key when non-empty
    ///   3. `~/.jackyzhang.app/token/jz.json` portal owner token
    ///
    /// base_url: `--base-url` → `$EASYBOOKS_API_URL` → config file → DEFAULT
    pub fn load(base_url_arg: Option<String>) -> Result<Self> {
        let file = read_config_file()?;
        let base_url = resolve_base_url(base_url_arg, file.as_ref().map(|c| c.base_url.clone()));

        if let Ok(env_key) = env::var("EASYBOOKS_API_KEY") {
            let env_key = env_key.trim().to_string();
            if !env_key.is_empty() {
                return Ok(Self::from_credential(env_key, base_url)?);
            }
        }

        if let Some(file) = file.as_ref() {
            if let Some(key) = file.api_key.as_ref() {
                let key = key.trim();
                if !key.is_empty() {
                    return Ok(Self::from_credential(key.to_string(), base_url)?);
                }
            }
        }

        if let Some(portal) = read_portal_owner_token()? {
            return Ok(Self {
                credential: portal,
                base_url,
                auth_kind: AuthKind::PortalOwner,
            });
        }

        bail!(
            "not logged in: run 'easybooks login --token-stdin' with a portal owner token (jz_) or legacy eb_live_ key (config: {})",
            config_path().unwrap_or_else(|_| "~/.easybooks/config.json".into())
        )
    }

    fn from_credential(credential: String, base_url: String) -> Result<Self> {
        let auth_kind = if credential.starts_with("jz_") {
            AuthKind::PortalOwner
        } else {
            AuthKind::ApiKey
        };
        Ok(Self {
            credential,
            base_url,
            auth_kind,
        })
    }

    /// Backward-compatible accessor used by existing call sites.
    #[allow(dead_code)]
    pub fn api_key(&self) -> &str {
        &self.credential
    }

    /// Masked form safe to print.
    pub fn api_key_masked(&self) -> String {
        mask_key(&self.credential)
    }
}

/// Resolve the effective `base_url` precedence used by both `Config::load` and
/// `login_from_stdin`.
pub fn resolve_base_url(base_url_arg: Option<String>, file_base_url: Option<String>) -> String {
    base_url_arg
        .filter(|u| !u.trim().is_empty())
        .or_else(|| env::var("EASYBOOKS_API_URL").ok().filter(|u| !u.trim().is_empty()))
        .or(file_base_url)
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

pub fn resolve_accountd_url() -> String {
    env::var("EASYBOOKS_ACCOUNTD_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .or_else(|| env::var("ANYPDF_ACCOUNTD_ORIGIN").ok().filter(|u| !u.trim().is_empty()))
        .unwrap_or_else(|| DEFAULT_ACCOUNTD_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn config_path() -> Result<String> {
    Ok(config_file_path()?.display().to_string())
}

pub fn config_path_buf() -> Result<PathBuf> {
    config_file_path()
}

fn config_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("HOME is not set")?;
    Ok(home.join(".easybooks").join("config.json"))
}

fn portal_token_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("HOME is not set")?;
    Ok(home.join(".jackyzhang.app").join("token").join("jz.json"))
}

fn read_config_file() -> Result<Option<ConfigFile>> {
    let path = config_file_path()?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("config path is not a regular file: {}", path.display());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("stat {}", path.display())),
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    // Backward compat: old shape had required string api_key.
    if let Ok(file) = serde_json::from_str::<ConfigFile>(&raw) {
        return Ok(Some(file));
    }
    #[derive(Deserialize)]
    struct Legacy {
        api_key: String,
        base_url: String,
    }
    let legacy: Legacy = serde_json::from_str(&raw)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(ConfigFile {
        api_key: Some(legacy.api_key),
        base_url: legacy.base_url,
    }))
}

/// Persist product-local runtime config (base_url ± legacy api_key). Never stores
/// portal owner tokens here.
pub fn save_config(api_key: Option<&str>, base_url: &str) -> Result<()> {
    let path = config_file_path()?;
    if let Some(parent) = path.parent() {
        if let Ok(metadata) = fs::symlink_metadata(parent) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("config directory is not a real directory: {}", parent.display());
            }
        } else {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("config path is not a regular file: {}", path.display());
        }
    }
    let file = ConfigFile {
        api_key: api_key
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        base_url: base_url.trim().to_string(),
    };
    let body = serde_json::to_vec_pretty(&file).context("serializing config")?;
    atomic_write(&path, &body, 0o600)?;
    Ok(())
}

/// Legacy helper used by older call sites: save eb_live_ key + base_url.
pub fn save(api_key: &str, base_url: &str) -> Result<()> {
    if api_key.starts_with("jz_") {
        save_portal_owner_token(api_key)?;
        // Keep base_url only under ~/.easybooks; owner token is shared.
        return save_config(None, base_url);
    }
    save_config(Some(api_key), base_url)
}

pub fn save_portal_owner_token(token: &str) -> Result<()> {
    let token = token.trim();
    if !token.starts_with("jz_") || token.chars().any(char::is_whitespace) {
        bail!("portal owner token must be a single jz_ value");
    }
    let path = portal_token_path()?;
    if let Some(parent) = path.parent() {
        if let Ok(metadata) = fs::symlink_metadata(parent) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("portal token directory is not a real directory: {}", parent.display());
            }
        } else {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            if let Some(root) = parent.parent() {
                let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
            }
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("portal token path is not a regular file: {}", path.display());
        }
    }
    let body = serde_json::to_vec(&serde_json::json!({ "token": token }))
        .context("serializing portal token")?;
    atomic_write(&path, &body, 0o600)?;
    Ok(())
}

pub fn read_portal_owner_token() -> Result<Option<String>> {
    let path = portal_token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    let token = value
        .get("token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.starts_with("jz_") && !s.is_empty())
        .map(|s| s.to_string());
    Ok(token)
}

fn atomic_write(path: &PathBuf, body: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("cfg"),
        std::process::id()
    ));
    {
        let mut file = fs::File::create(&tmp)
            .with_context(|| format!("creating temp file {}", tmp.display()))?;
        file.write_all(body)
            .and_then(|_| file.flush())
            .context("writing temp config")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))
                .context("setting temp permissions")?;
        }
    }
    fs::rename(&tmp, path).with_context(|| format!("persisting {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    Ok(())
}

pub fn mask_key(key: &str) -> String {
    if key.starts_with("jz_") {
        return "jz_***".to_string();
    }
    if key.starts_with("eb_") {
        return "eb_***".to_string();
    }
    if key.len() <= 4 {
        return "***".to_string();
    }
    format!("{}***", &key[..key.len().min(3)])
}
