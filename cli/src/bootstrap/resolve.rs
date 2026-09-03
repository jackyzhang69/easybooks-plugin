//! Resolver for the `easybooks` binary path + plugin-cache freshness.
//!
//! Resolution order (contract §0) — first existing executable wins:
//!   1. `$EASYBOOKS_BIN`                                  (explicit override)
//!   2. `$CLAUDE_PLUGIN_ROOT/bin/<platform>/easybooks`    (Claude Code)
//!   3. Codex cache:
//!      `$HOME/.codex/plugins/cache/jacky-plugins/easybooks/<highest-version>/bin/<platform>/easybooks`
//!   4. `command -v easybooks`                            (manual PATH install)
//!
//! `<platform>` ∈ darwin-arm64, darwin-x64, linux-x64, win32-x64 (binary
//! `easybooks.exe` on win32-x64).

use std::path::{Path, PathBuf};

/// Bare binary name for the current OS (`.exe` suffix on Windows).
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "easybooks.exe"
    } else {
        "easybooks"
    }
}

/// Plugin `<platform>` token for the running host, per contract §0.
/// Returns `None` for an unsupported os/arch pair.
pub fn current_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-x64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("windows", "x86_64") => Some("win32-x64"),
        _ => None,
    }
}

/// Resolve the path to the `easybooks` binary using the contract §0 order.
/// Returns the first existing executable. This is the same logic each skill's
/// §B block performs in shell; exposed here so `doctor` (and tests) can assert
/// against it without shelling out.
pub fn resolve_binary() -> Option<PathBuf> {
    let name = binary_name();
    let plat = current_platform();

    // 1. Explicit override.
    if let Ok(explicit) = std::env::var("EASYBOOKS_BIN") {
        if !explicit.is_empty() {
            let p = PathBuf::from(explicit);
            if is_file(&p) {
                return Some(p);
            }
        }
    }

    // 2. Claude Code plugin root.
    if let (Ok(root), Some(plat)) = (std::env::var("CLAUDE_PLUGIN_ROOT"), plat) {
        if !root.is_empty() {
            let p = PathBuf::from(root).join("bin").join(plat).join(name);
            if is_file(&p) {
                return Some(p);
            }
        }
    }

    // 3. Codex cache — highest version dir wins.
    if let (Some(home), Some(plat)) = (dirs::home_dir(), plat) {
        let plugin_dir = home
            .join(".codex")
            .join("plugins")
            .join("cache")
            .join("jacky-plugins")
            .join("easybooks");
        if let Some(version_dir) = highest_version_dir(&plugin_dir) {
            let p = version_dir.join("bin").join(plat).join(name);
            if is_file(&p) {
                return Some(p);
            }
        }
    }

    // 4. PATH lookup (`command -v easybooks`).
    if let Some(p) = which_on_path(name) {
        return Some(p);
    }

    None
}

/// Find the highest strict-semver subdir under `plugin_dir`. Used to pick the
/// active version in the Codex cache layout.
fn highest_version_dir(plugin_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(plugin_dir).ok()?;
    let mut versions: Vec<(Vec<u32>, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            parse_strict_semver(&name).map(|v| (v, p))
        })
        .collect();
    versions.sort_by(|a, b| a.0.cmp(&b.0));
    versions.pop().map(|(_, p)| p)
}

/// Minimal `command -v` equivalent: scan `$PATH` entries for an executable
/// named `name`. On unix we additionally require the execute bit.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_file(p: &Path) -> bool {
    p.is_file()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

// ---------------------------------------------------------------------------
// Cache freshness (doctor's `cache` block, contract §2).
// ---------------------------------------------------------------------------

/// Status of the running binary relative to its plugin cache, if any.
pub enum CacheStatus {
    NotInCache,
    Fresh {
        kind: &'static str,
        version: String,
    },
    Stale {
        kind: &'static str,
        current: String,
        latest: String,
        latest_path: PathBuf,
    },
}

impl CacheStatus {
    /// JSON form matching contract §2's `cache` shape:
    ///   not-in-cache → {"location":"not_in_cache","stale":false}
    ///   fresh        → {"location":<kind>,"version":<v>,"stale":false,"latest_available":<v>}
    ///   stale        → {"location":<kind>,"version":<cur>,"stale":true,"latest_available":<latest>, ...}
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            CacheStatus::NotInCache => serde_json::json!({
                "location": "not_in_cache",
                "stale": false,
            }),
            CacheStatus::Fresh { kind, version } => serde_json::json!({
                "location": kind,
                "version": version,
                "stale": false,
                "latest_available": version,
            }),
            CacheStatus::Stale {
                kind,
                current,
                latest,
                latest_path,
            } => serde_json::json!({
                "location": kind,
                "version": current,
                "stale": true,
                "latest_available": latest,
                "latest_path": latest_path.display().to_string(),
                "remediation": "refresh plugin cache (codex: re-sync jacky-plugins; claude: reinstall) or delete the stale version dir",
            }),
        }
    }
}

/// If the running binary lives under a codex / claude plugin cache, scan
/// sibling version dirs and report whether a newer one exists. Layout:
///   <cache_root>/<plugin_name>/<version>/bin/<platform>/easybooks
pub fn detect_cache_status(self_exe: Option<&Path>, self_version: &str) -> CacheStatus {
    let Some(exe) = self_exe else {
        return CacheStatus::NotInCache;
    };
    // ancestors: exe, bin/<platform>, bin, <version>, <plugin>, cache_root, ...
    let ancestors: Vec<&Path> = exe.ancestors().collect();
    if ancestors.len() < 6 {
        return CacheStatus::NotInCache;
    }
    let version_dir = ancestors[3];
    let plugin_dir = ancestors[4];
    let cache_root = ancestors[5];

    let kind = detect_kind(cache_root);
    if kind.is_empty() {
        return CacheStatus::NotInCache;
    }

    let current = version_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(self_version)
        .to_string();

    let Ok(entries) = std::fs::read_dir(plugin_dir) else {
        return CacheStatus::Fresh {
            kind,
            version: current,
        };
    };

    let platform = ancestors[1]
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let bin_name = binary_name();

    let mut versions: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| {
            let p = e.path();
            let n = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            (n, p)
        })
        // Strict semver names only — rejects `.tmp-foo`, `99.99.99-evil`, etc.
        .filter(|(n, _)| parse_strict_semver(n).is_some())
        // The candidate dir must actually contain a same-platform binary,
        // not just be an empty / partial layout.
        .filter(|(_, p)| p.join("bin").join(platform).join(bin_name).is_file())
        .collect();
    versions.sort_by(|a, b| semver_cmp(&a.0, &b.0));

    let Some((latest_name, latest_path)) = versions.last() else {
        return CacheStatus::Fresh {
            kind,
            version: current,
        };
    };
    if latest_name == &current {
        CacheStatus::Fresh {
            kind,
            version: current,
        }
    } else {
        CacheStatus::Stale {
            kind,
            current,
            latest: latest_name.clone(),
            latest_path: latest_path.clone(),
        }
    }
}

fn detect_kind(cache_root: &Path) -> &'static str {
    let mut codex = false;
    let mut claude = false;
    let mut prev: Option<&std::ffi::OsStr> = None;
    let mut prev_prev: Option<&std::ffi::OsStr> = None;
    for comp in cache_root.components() {
        let s = match comp {
            std::path::Component::Normal(s) => s,
            _ => continue,
        };
        if s == "cache" {
            if let (Some(pp), Some(p)) = (prev_prev, prev) {
                if p == "plugins" && pp == ".codex" {
                    codex = true;
                } else if p == "plugins" && pp == ".claude" {
                    claude = true;
                }
            }
        }
        prev_prev = prev;
        prev = Some(s);
    }
    if codex {
        "codex"
    } else if claude {
        "claude"
    } else {
        ""
    }
}

/// Strict semver-ish parse: clean dotted-decimal only (2..=4 components).
/// Rejects adversarial / leftover dirs like `.tmp-foo` or `99.99.99-x`.
pub fn parse_strict_semver(s: &str) -> Option<Vec<u32>> {
    let parts: Vec<&str> = s.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out.push(p.parse::<u32>().ok()?);
    }
    Some(out)
}

fn semver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa = parse_strict_semver(a).unwrap_or_default();
    let pb = parse_strict_semver(b).unwrap_or_default();
    pa.cmp(&pb)
}

/// Replace the user's $HOME with `~` in a string. Best-effort — keeps
/// diagnostic JSON portable / non-PII when forwarded.
pub fn redact_home(s: &str) -> String {
    match dirs::home_dir() {
        Some(h) => {
            let hs = h.to_string_lossy();
            if hs.is_empty() {
                s.to_string()
            } else {
                s.replace(hs.as_ref(), "~")
            }
        }
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_semver_accepts_dotted_and_rejects_junk() {
        assert_eq!(parse_strict_semver("0.1.0"), Some(vec![0, 1, 0]));
        assert_eq!(parse_strict_semver("1.2"), Some(vec![1, 2]));
        assert_eq!(parse_strict_semver("1.2.3.4"), Some(vec![1, 2, 3, 4]));
        assert_eq!(parse_strict_semver("0.1.0-rc1"), None);
        assert_eq!(parse_strict_semver(".tmp"), None);
        assert_eq!(parse_strict_semver("v1.2.3"), None);
        assert_eq!(parse_strict_semver("1"), None);
    }

    #[test]
    fn platform_token_is_known_or_none() {
        // On any host the test runs on, the token (if Some) must be one of the
        // four contract platforms.
        if let Some(p) = current_platform() {
            assert!(["darwin-arm64", "darwin-x64", "linux-x64", "win32-x64"].contains(&p));
        }
    }

    #[test]
    fn not_in_cache_for_shallow_path() {
        let p = Path::new("/usr/local/bin/easybooks");
        assert!(matches!(
            detect_cache_status(Some(p), "0.1.0"),
            CacheStatus::NotInCache
        ));
    }
}
