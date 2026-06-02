//! Request-shape contract tests for the EasyBooks CLI.
//!
//! These assert the EXACT request the CLI sends to the backend integration
//! endpoints (contract §3) — method, path, headers, and JSON body — against a
//! mockito server. They never hit a real backend.
//!
//! Auth model (v2): every request carries `Authorization: Bearer <api_key>`.
//! The api_key is the user's personal EasyBooks key; it both authenticates and
//! identifies the user, so there is no owner-id header or body field.
//!
//! Config is supplied via env (`EASYBOOKS_API_KEY` / `EASYBOOKS_API_URL`) so the
//! tests are hermetic and don't touch `~/.easybooks/config.json`.

use assert_cmd::Command;
use mockito::Matcher;
use predicates::prelude::*;

fn easybooks() -> Command {
    Command::cargo_bin("easybooks").expect("easybooks binary")
}

const KEY: &str = "eb_live_test_key";
const BEARER: &str = "Bearer eb_live_test_key";
const USER: &str = "11111111-1111-1111-1111-111111111111";

#[test]
fn expense_add_posts_single_entry_with_bearer_and_cents() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/ingest/transactions")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{
                    "source_system": "manual",
                    "entries": [
                        {
                            "type": "expense",
                            "amount_cents": 12000,
                            "description": "Software subscription",
                            "date": "2026-05-01",
                            "category_name": "Software",
                            "classification": "business",
                            "source_id": "rcpt-123"
                        }
                    ]
                }"#
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ok","created":1,"existing":0,"skipped":0,"processed":1}"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "expense",
            "add",
            "--amount",
            "120.00",
            "--description",
            "Software subscription",
            "--date",
            "2026-05-01",
            "--category",
            "Software",
            "--classification",
            "business",
            "--source-id",
            "rcpt-123",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""created": 1"#));

    mock.assert();
}

#[test]
fn income_add_dry_run_does_not_post_and_echoes_cents() {
    // No mock endpoint should be hit on a dry run. We still point at a server
    // URL; if the CLI erroneously POSTed, mockito would record an unmatched
    // request and the body assertions below would never appear.
    let server = mockito::Server::new();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "income",
            "add",
            "--amount",
            "1,234.56",
            "--description",
            "Consulting fee",
            "--date",
            "2026-04-15",
            "--source-id",
            "inv-9",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status": "dry_run""#))
        .stdout(predicate::str::contains(r#""amount_cents": 123456"#))
        .stdout(predicate::str::contains(r#""type": "income""#));
}

#[test]
fn tx_import_json_posts_envelope_verbatim() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/ingest/transactions")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{
                    "source_system": "stripe",
                    "entries": [
                        {
                            "type": "income",
                            "amount_cents": 5000,
                            "description": "Payout",
                            "date": "2026-03-01",
                            "source_id": "po_1"
                        },
                        {
                            "type": "expense",
                            "amount_cents": 250,
                            "description": "Fee",
                            "date": "2026-03-01",
                            "source_id": "fee_1"
                        }
                    ]
                }"#
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ok","created":2,"existing":0,"skipped":0,"processed":2}"#)
        .create();

    let payload = r#"{"source_system":"stripe","entries":[{"type":"income","amount_cents":5000,"description":"Payout","date":"2026-03-01","source_id":"po_1"},{"type":"expense","amount_cents":250,"description":"Fee","date":"2026-03-01","source_id":"fee_1"}]}"#;

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["tx", "import-json", "--json", payload])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""processed": 2"#));

    mock.assert();
}

#[test]
fn tx_import_json_dry_run_validates_without_posting() {
    let server = mockito::Server::new();
    let payload = r#"{"source_system":"manual","entries":[{"type":"expense","amount_cents":999,"description":"X","date":"2026-01-01","source_id":"a"}]}"#;

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["tx", "import-json", "--json", payload, "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status": "dry_run""#))
        .stdout(predicate::str::contains(r#""amount_cents": 999"#));
}

#[test]
fn tx_import_json_rejects_entry_missing_source_id() {
    let server = mockito::Server::new();
    // Missing source_id → local validation fails BEFORE any network call.
    let payload = r#"{"source_system":"manual","entries":[{"type":"expense","amount_cents":100,"description":"X","date":"2026-01-01"}]}"#;

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["tx", "import-json", "--json", payload, "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("source_id"));
}

#[test]
fn gmail_record_defaults_source_system_to_gmail() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/ingest/transactions")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{
                    "source_system": "gmail",
                    "entries": [
                        {
                            "type": "expense",
                            "amount_cents": 4200,
                            "description": "AWS",
                            "date": "2026-02-02",
                            "source_id": "msg-abc"
                        }
                    ]
                }"#
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ok","created":1,"existing":0,"skipped":0,"processed":1}"#)
        .create();

    // Note: no source_system in the input JSON — gmail record must default it.
    let payload = r#"{"entries":[{"type":"expense","amount_cents":4200,"description":"AWS","date":"2026-02-02","source_id":"msg-abc"}]}"#;

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["gmail", "record", "--json", payload])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""created": 1"#));

    mock.assert();
}

#[test]
fn gmail_sync_is_v1_stub() {
    easybooks()
        .env("EASYBOOKS_API_URL", "http://127.0.0.1:9")
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["gmail", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status": "not_implemented_v1""#));
}

#[test]
fn login_writes_config_and_masks_key() {
    let home = tempfile::tempdir().expect("tempdir");
    easybooks()
        .env("HOME", home.path())
        .args([
            "login",
            "--token",
            "eb_live_super_secret_value",
            "--base-url",
            "http://192.168.1.98:8310",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""api_key_masked": "eb_***""#))
        // The raw secret must NEVER appear in output.
        .stdout(predicate::str::contains("eb_live_super_secret_value").not());

    // Config file exists and contains the key (on disk only).
    let cfg = home.path().join(".easybooks").join("config.json");
    assert!(cfg.is_file(), "config.json should be written");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.json must be mode 0600");
    }
}

/// Read back the `base_url` persisted to `<home>/.easybooks/config.json`.
/// Tests run the binary with an isolated `HOME`, so this is the exact value
/// `login` wrote to disk (not the echoed stdout, though they must agree).
fn persisted_base_url(home: &std::path::Path) -> String {
    let cfg = home.join(".easybooks").join("config.json");
    let data = std::fs::read_to_string(&cfg).expect("config.json should exist after login");
    let json: serde_json::Value = serde_json::from_str(&data).expect("config.json is valid JSON");
    json["base_url"]
        .as_str()
        .expect("base_url is a string")
        .to_string()
}

const TEST_URL: &str = "https://easybooks-test.jackyzhang.app";
const PROD_URL: &str = "https://easybooks.jackyzhang.app";

/// login precedence (a): `$EASYBOOKS_API_URL` set + no `--base-url` →
/// persist the env value, NOT the PROD default. This is the governance footgun
/// the item-3 fix closes: previously the clap default silently won over env.
#[test]
fn login_uses_env_base_url_when_arg_absent() {
    let home = tempfile::tempdir().expect("tempdir");
    easybooks()
        .env("HOME", home.path())
        .env("EASYBOOKS_API_URL", TEST_URL)
        .args(["login", "--token", "eb_live_envonly"])
        .assert()
        .success()
        .stdout(predicate::str::contains(TEST_URL));

    assert_eq!(
        persisted_base_url(home.path()),
        TEST_URL,
        "with EASYBOOKS_API_URL set and no --base-url, login must persist the env value"
    );
}

/// login precedence (b): `--base-url` arg present overrides `$EASYBOOKS_API_URL`.
#[test]
fn login_arg_overrides_env_base_url() {
    let home = tempfile::tempdir().expect("tempdir");
    let arg_url = "http://192.168.1.98:8310";
    easybooks()
        .env("HOME", home.path())
        .env("EASYBOOKS_API_URL", TEST_URL)
        .args(["login", "--token", "eb_live_argwins", "--base-url", arg_url])
        .assert()
        .success()
        .stdout(predicate::str::contains(arg_url));

    assert_eq!(
        persisted_base_url(home.path()),
        arg_url,
        "--base-url must override EASYBOOKS_API_URL"
    );
}

/// login precedence (c): neither `--base-url` nor `$EASYBOOKS_API_URL` →
/// fall back to the PROD DEFAULT.
#[test]
fn login_defaults_to_prod_when_arg_and_env_absent() {
    let home = tempfile::tempdir().expect("tempdir");
    easybooks()
        .env("HOME", home.path())
        .env_remove("EASYBOOKS_API_URL")
        .args(["login", "--token", "eb_live_default"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PROD_URL));

    assert_eq!(
        persisted_base_url(home.path()),
        PROD_URL,
        "with neither arg nor env, login must persist the PROD DEFAULT"
    );
}

#[test]
fn whoami_uses_integration_whoami_endpoint_and_reports_user_and_scope() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/integrations/whoami")
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"ok":true,"user_id":"{USER}","scope":"read_write","source":"easybooks-integration"}}"#
        ))
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["whoami"])
        .assert()
        .success()
        .stdout(predicate::str::contains(USER))
        .stdout(predicate::str::contains(r#""scope": "read_write""#))
        .stdout(predicate::str::contains(r#""api_key_masked": "eb_***""#));

    mock.assert();
}

#[test]
fn categories_list_hits_categories_endpoint_with_type_and_bearer() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/integrations/categories")
        .match_query(Matcher::UrlEncoded("type".into(), "expense".into()))
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["categories", "list", "--type", "expense"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));

    mock.assert();
}

#[test]
fn invoice_create_dry_run_does_not_post() {
    let server = mockito::Server::new();
    let payload = r#"{"client":{"name":"Acme"},"issue_date":"2026-05-01","due_date":"2026-06-01","items":[{"description":"Work","quantity":1,"unit_price":100}]}"#;

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["invoice", "create", "--json", payload, "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status": "dry_run""#))
        .stdout(predicate::str::contains("Acme"));
}

#[test]
fn invoice_send_posts_to_integration_route_with_bearer() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/invoice/inv_42/send")
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"sent":true}"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["invoice", "send", "inv_42"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""sent": true"#));

    mock.assert();
}

#[test]
fn missing_config_is_a_structured_error() {
    let home = tempfile::tempdir().expect("tempdir");
    // No config file, no env key → "not logged in" error on stderr, non-zero.
    easybooks()
        .env("HOME", home.path())
        .env_remove("EASYBOOKS_API_KEY")
        .env_remove("EASYBOOKS_API_URL")
        .args(["categories", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(r#""error""#))
        .stderr(predicate::str::contains("not logged in"));
}
