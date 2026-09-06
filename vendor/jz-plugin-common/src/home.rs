//! Platform home paths and atomic private file writes.
//!
//! Root: `~/.jackyzhang.app` or `$JACKYZHANG_APP_HOME`.
//! Shared user slot: `<root>/token/user.json` (0600).
//! Plugin runtime: `<root>/<plugin_id>/` (0700) — feedback mirror at `feedback.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const PLATFORM_HOME_ENV: &str = "JACKYZHANG_APP_HOME";
const DURABLE_TOKEN_PREFIX: &str = "jz_";

/// Resolved platform home layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub platform: PathBuf,
}

impl Home {
    pub fn resolve() -> Result<Self, io::Error> {
        Ok(Self {
            platform: platform_home()?,
        })
    }

    pub fn token_dir(&self) -> PathBuf {
        self.platform.join("token")
    }

    pub fn user_token_path(&self) -> PathBuf {
        self.token_dir().join("user.json")
    }

    /// Plugin runtime directory: `<platform>/<plugin_id>/`.
    pub fn plugin_runtime_dir(&self, plugin_id: &str) -> PathBuf {
        self.platform.join(plugin_id)
    }

    /// Install tree root: `<platform>/plugins/<plugin_id>/`.
    pub fn plugin_install_root(&self, plugin_id: &str) -> PathBuf {
        self.platform.join("plugins").join(plugin_id)
    }

    pub fn feedback_mirror_path(&self, plugin_id: &str) -> PathBuf {
        self.plugin_runtime_dir(plugin_id).join("feedback.json")
    }

    pub fn ensure_plugin_runtime(&self, plugin_id: &str) -> Result<PathBuf, io::Error> {
        let dir = self.plugin_runtime_dir(plugin_id);
        fs::create_dir_all(&dir)?;
        set_private_dir(&dir)?;
        Ok(dir)
    }

    pub fn ensure_token_dir(&self) -> Result<(), io::Error> {
        guard_credential_paths(self)?;
        fs::create_dir_all(self.token_dir())?;
        set_private_dir(&self.token_dir())?;
        set_private_dir(&self.platform)?;
        Ok(())
    }
}

pub fn platform_home() -> Result<PathBuf, io::Error> {
    if let Ok(raw) = std::env::var(PLATFORM_HOME_ENV) {
        if !raw.is_empty() {
            return Ok(PathBuf::from(raw));
        }
    }
    default_platform_home()
}

fn default_platform_home() -> Result<PathBuf, io::Error> {
    dirs_home()
        .map(|home| home.join(".jackyzhang.app"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Fail closed before credential read/write: platform home and token dir must be real directories.
pub fn guard_credential_paths(home: &Home) -> Result<(), io::Error> {
    if home.platform.exists() {
        ensure_real_directory(&home.platform)?;
    }
    let token_dir = home.token_dir();
    if token_dir.exists() {
        ensure_real_directory(&token_dir)?;
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<(), io::Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("directory is a symlink: {}", path.display()),
        )),
        Ok(meta) if !meta.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        )),
        Ok(_) => Ok(()),
    }
}

/// Fail closed on symlinks for credential paths.
pub fn reject_symlink(path: &Path) -> Result<(), io::Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing symlink at {}", path.display()),
        )),
        Ok(_) => Ok(()),
    }
}

/// Atomic write with mode 0600 on Unix.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    let home = Home::resolve()?;
    guard_credential_paths(&home)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    if parent.exists() {
        ensure_real_directory(parent)?;
    }
    fs::create_dir_all(parent)?;
    set_private_dir(parent)?;
    reject_symlink(path)?;
    let tmp_name = format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tmp")
    );
    let tmp = parent.join(tmp_name);
    let _ = fs::remove_file(&tmp);
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        if !bytes.ends_with(b"\n") {
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn set_private_dir(path: &Path) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserSlot {
    token: String,
    #[serde(default = "default_user")]
    credential_kind: String,
    #[serde(default = "default_user")]
    slot: String,
}

fn default_user() -> String {
    "user".to_string()
}

/// Read the shared durable user credential from `token/user.json`.
pub fn read_durable_token() -> Result<Option<String>, io::Error> {
    let home = Home::resolve()?;
    guard_credential_paths(&home)?;
    let path = home.user_token_path();
    if !path.is_file() {
        return Ok(None);
    }
    reject_symlink(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(&path)?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Ok(None);
        }
    }
    let raw = fs::read_to_string(&path)?;
    let parsed: UserSlot = serde_json::from_str(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse user slot: {error}"),
        )
    })?;
    let token = parsed.token.trim();
    if token.is_empty()
        || !token.starts_with(DURABLE_TOKEN_PREFIX)
        || token.chars().any(char::is_whitespace)
    {
        return Ok(None);
    }
    if parsed.credential_kind != "user" || parsed.slot != "user" {
        return Ok(None);
    }
    Ok(Some(token.to_string()))
}

/// Write the shared durable user credential atomically (0600).
pub fn write_durable_token(token: &str) -> Result<(), io::Error> {
    let token = token.trim();
    if token.is_empty()
        || !token.starts_with(DURABLE_TOKEN_PREFIX)
        || token.chars().any(char::is_whitespace)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid durable token shape",
        ));
    }
    let home = Home::resolve()?;
    home.ensure_token_dir()?;
    let payload = UserSlot {
        token: token.to_string(),
        credential_kind: "user".to_string(),
        slot: "user".to_string(),
    };
    let bytes = serde_json::to_vec(&payload).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("encode user slot: {error}"),
        )
    })?;
    write_private_file(&home.user_token_path(), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env;

    #[test]
    fn atomic_write_and_layout() {
        let _guard = test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var(PLATFORM_HOME_ENV, tmp.path());
        write_durable_token("jz_test_home_layout").unwrap();
        let home = Home::resolve().unwrap();
        assert_eq!(
            home.user_token_path(),
            tmp.path().join("token").join("user.json")
        );
        assert_eq!(
            home.feedback_mirror_path("anydoc"),
            tmp.path().join("anydoc").join("feedback.json")
        );
        assert_eq!(
            home.plugin_install_root("anydoc"),
            tmp.path().join("plugins").join("anydoc")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(home.user_token_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        assert_eq!(
            read_durable_token().unwrap().as_deref(),
            Some("jz_test_home_layout")
        );
        std::env::remove_var(PLATFORM_HOME_ENV);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_token_dir_rejects_read_and_write() {
        let _guard = test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        std::env::set_var(PLATFORM_HOME_ENV, tmp.path());
        std::os::unix::fs::symlink(real.path(), tmp.path().join("token")).unwrap();
        assert!(write_durable_token("jz_symlink_dir").is_err());
        assert!(read_durable_token().is_err());
        std::env::remove_var(PLATFORM_HOME_ENV);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_token_file_rejects_read_and_write() {
        let _guard = test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var(PLATFORM_HOME_ENV, tmp.path());
        fs::create_dir_all(tmp.path().join("token")).unwrap();
        let real = tmp.path().join("real-user.json");
        fs::write(
            &real,
            br#"{"token":"jz_x","credential_kind":"user","slot":"user"}"#,
        )
        .unwrap();
        let alias = tmp.path().join("token").join("user.json");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        assert!(write_durable_token("jz_symlink_file").is_err());
        assert!(read_durable_token().is_err());
        std::env::remove_var(PLATFORM_HOME_ENV);
    }

    #[cfg(unix)]
    #[test]
    fn loose_token_permissions_are_refused() {
        let _guard = test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var(PLATFORM_HOME_ENV, tmp.path());
        write_durable_token("jz_loose_perms").unwrap();
        let path = Home::resolve().unwrap().user_token_path();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_durable_token().unwrap(), None);
        std::env::remove_var(PLATFORM_HOME_ENV);
    }
}
