use std::path::PathBuf;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn plugin_identity_gate_is_wired() {
    let root = crate_root();
    let script = root.join("scripts/official-plugin/verify-plugin-identity.sh");
    assert!(
        script.is_file(),
        "scripts/official-plugin/verify-plugin-identity.sh must exist"
    );

    let pin = std::fs::read_to_string(root.join("scripts/official-plugin/PLATFORM_GOVERNANCE_REV"))
        .expect("PLATFORM_GOVERNANCE_REV must exist");
    let pin = pin.trim();
    assert_eq!(pin.len(), 40, "pin must be 40 hex chars");
    assert!(
        pin.chars().all(|c| c.is_ascii_hexdigit()),
        "pin must be hexadecimal: {pin}"
    );

    let ci =
        std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci.yml must exist");
    assert!(
        ci.contains("verify-plugin-identity.sh"),
        "ci.yml must invoke verify-plugin-identity.sh"
    );

    let publish = std::fs::read_to_string(root.join(".github/workflows/publish.yml"))
        .expect("publish.yml must exist");
    assert!(
        publish.contains("verify-plugin-identity.sh"),
        "publish.yml must invoke verify-plugin-identity.sh"
    );
}
