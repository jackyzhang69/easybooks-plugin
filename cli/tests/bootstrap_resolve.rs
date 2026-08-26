//! Unit-level checks for the binary/version resolver (contract §0) exercised
//! through the public lib surface. These are pure path/logic assertions — no
//! network, no real plugin cache required.

use easybooks_cli::bootstrap;
use std::path::Path;

#[test]
fn shallow_install_path_is_not_in_cache() {
    // A manual PATH install (e.g. /usr/local/bin/easybooks) is too shallow to
    // be a plugin-cache layout, so cache status must be NotInCache → its JSON
    // reports location "not_in_cache".
    let p = Path::new("/usr/local/bin/easybooks");
    let status = bootstrap::detect_cache_status(Some(p), "0.1.0");
    let json = status.to_json();
    assert_eq!(
        json.get("location").and_then(|v| v.as_str()),
        Some("not_in_cache")
    );
    assert_eq!(json.get("stale").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn codex_cache_layout_reports_codex_location() {
    // Synthesise the codex cache layout:
    //   .../.codex/plugins/cache/jacky-plugins/easybooks/0.1.0/bin/<plat>/easybooks
    // ancestors[5] is the cache_root containing `.codex/plugins/cache`.
    let Some(plat) = bootstrap::current_platform() else {
        // Unsupported host platform; nothing to assert.
        return;
    };
    let exe = format!(
        "/home/u/.codex/plugins/cache/jacky-plugins/easybooks/0.1.0/bin/{plat}/easybooks"
    );
    let status = bootstrap::detect_cache_status(Some(Path::new(&exe)), "0.1.0");
    let json = status.to_json();
    // Even though the sibling-version scan finds no real dirs (path doesn't
    // exist), the location must be detected as the codex cache and treated as
    // fresh (no newer sibling present).
    assert_eq!(
        json.get("location").and_then(|v| v.as_str()),
        Some("codex")
    );
    assert_eq!(json.get("stale").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn explicit_override_env_resolves_when_file_exists() {
    // $EASYBOOKS_BIN pointing at a real executable wins (resolution step 1).
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("easybooks-fake");
    std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // SAFETY: single-threaded test; we set + clear the override deterministically.
    std::env::set_var("EASYBOOKS_BIN", &bin);
    let resolved = bootstrap::resolve_binary();
    std::env::remove_var("EASYBOOKS_BIN");

    assert_eq!(resolved.as_deref(), Some(bin.as_path()));
}
