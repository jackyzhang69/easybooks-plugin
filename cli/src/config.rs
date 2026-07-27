use anyhow::{Context, Result};
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

/// On-disk shape of `~/.easybooks/config.json`. The `api_key` is the user's
/// personal EasyBooks API key (`eb_live_...`), sent as `Authorization: Bearer
/// <api_key>` on every request. It both authenticates AND identifies the user —
/// there is no separate owner id. It is NEVER printed — see `mask_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
}

impl Config {
    /// Resolve effective config. Precedence for each field:
    ///   api_key:  $EASYBOOKS_API_KEY env → config file
    ///   base_url: --base-url arg → $EASYBOOKS_API_URL env → config file → DEFAULT
    ///             (DEFAULT is PROD: https://easybooks.jackyzhang.app)
    /// If no api_key can be resolved anywhere, this is a hard "not logged in"
    /// error pointing the user at `easybooks login`.
    pub fn load(base_url_arg: Option<String>) -> Result<Self> {
        let file = read_config_file()?;

        let api_key = env::var("EASYBOOKS_API_KEY")
            .ok()
            .or_else(|| file.as_ref().map(|c| c.api_key.clone()))
            .filter(|k| !k.is_empty())
            .with_context(|| {
                format!(
                    "not logged in: run 'easybooks login --token-stdin' (config: {})",
                    config_path().unwrap_or_else(|_| "~/.easybooks/config.json".into())
                )
            })?;

        let base_url = resolve_base_url(base_url_arg, file.as_ref().map(|c| c.base_url.clone()));

        Ok(Self { api_key, base_url })
    }

    /// Masked form of the key, safe to print anywhere: `eb_***`.
    pub fn api_key_masked(&self) -> String {
        mask_key(&self.api_key)
    }
}

/// Resolve the effective `base_url` per the documented precedence (contract §6):
///   --base-url arg → $EASYBOOKS_API_URL env → file/config fallback → DEFAULT (PROD).
///
/// `file_fallback` is the config-file tier (`Config::load` passes the persisted
/// base_url; `login` passes `None` because it is *writing* the config and must
/// not read its own stale value back). Empty strings are treated as unset so a
/// blank arg/env/file never wins over a real later tier.
pub fn resolve_base_url(base_url_arg: Option<String>, file_fallback: Option<String>) -> String {
    base_url_arg
        .or_else(|| env::var("EASYBOOKS_API_URL").ok())
        .or(file_fallback)
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

/// Mask the API key as `eb_***`. The full value is never emitted in any output
/// (contract §2). We deliberately reveal NOTHING beyond the brand prefix.
pub fn mask_key(_key: &str) -> String {
    "eb_***".to_string()
}

fn read_config_file() -> Result<Option<Config>> {
    let path = config_path()?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            anyhow::bail!("config path is not a regular file: {}", path);
        }
    }
    match fs::read_to_string(&path) {
        Ok(data) => {
            let cfg: Config = serde_json::from_str(&data)
                .with_context(|| format!("invalid config JSON at {}", path))?;
            Ok(Some(cfg))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading config at {}", path)),
    }
}

/// Path to `~/.easybooks/config.json` as a display string.
pub fn config_path() -> Result<String> {
    Ok(config_path_buf()?.to_string_lossy().to_string())
}

pub fn config_path_buf() -> Result<PathBuf> {
    let mut path =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    path.push(".easybooks");
    path.push("config.json");
    Ok(path)
}

/// Persist config to `~/.easybooks/config.json` with mode 0600 on unix.
/// Creates the parent dir (also 0700 on unix) if missing.
pub fn save(api_key: &str, base_url: &str) -> Result<()> {
    let path = config_path_buf()?;
    if let Some(parent) = path.parent() {
        if let Ok(metadata) = fs::symlink_metadata(parent) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!("config directory is not a real directory: {}", parent.display());
            }
        } else {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let cfg = Config {
        api_key: api_key.to_string(),
        base_url: base_url.to_string(),
    };
    let data = serde_json::to_vec_pretty(&cfg)?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            anyhow::bail!("config path is not a regular file: {}", path.display());
        }
    }
    let temporary = path.with_file_name(format!(
        ".config.json.{}.tmp",
        std::process::id()
    ));
    if temporary.exists() {
        anyhow::bail!("temporary config path already exists");
    }
    let write_result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("creating temporary config beside {}", path.display()))?;
        file.write_all(&data)
            .context("writing temporary EasyBooks config")?;
        file.sync_all().context("syncing temporary EasyBooks config")?;
        fs::rename(&temporary, &path)
            .with_context(|| format!("replacing config at {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting 0600 on {}", path.display()))?;
    }
    Ok(())
}
