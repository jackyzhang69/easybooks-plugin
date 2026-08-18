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

const KEY: &str = "jz_test_key_for_contract";
const BEARER: &str = "Bearer jz_test_key_for_contract";
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
fn top_level_json_flag_precedes_command_contract_matrix() {
    let home = tempfile::tempdir().expect("tempdir");
    easybooks()
        .env("HOME", home.path())
        .args(["--json", "doctor", "--no-fetch"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""binary_version""#));

    for command in ["login", "whoami"] {
        easybooks()
            .args(["--json", command, "--help"])
            .assert()
            .success();
    }

    easybooks()
        .args(["doctor", "--json", "--no-fetch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument '--json'"));
}

#[test]
fn login_writes_config_and_masks_key() {
    let home = tempfile::tempdir().expect("tempdir");
    easybooks()
        .env("HOME", home.path())
        .args(["login", "--token-stdin", "--base-url", "http://192.168.1.69:8310"])
        .write_stdin("jz_super_secret_value\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""api_key_masked": "jz_***""#))
        // The raw secret must NEVER appear in output.
        .stdout(predicate::str::contains("jz_super_secret_value").not());

    // Shared user slot holds the token; product runtime config has base_url only.
    let slot = home.path().join(".jackyzhang.app").join("token").join("user.json");
    assert!(slot.is_file(), "shared user.json should be written");
    let slot_raw = std::fs::read_to_string(&slot).expect("read user.json");
    assert!(slot_raw.contains("jz_super_secret_value"));
    let cfg = home.path().join(".easybooks").join("config.json");
    assert!(cfg.is_file(), "runtime config.json should be written");
    let cfg_raw = std::fs::read_to_string(&cfg).expect("read runtime config");
    assert!(!cfg_raw.contains("jz_super_secret_value"), "token must not live in product config");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.json must be mode 0600");
        let directory_mode = std::fs::metadata(cfg.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700, ".easybooks must be mode 0700");
        let slot_mode = std::fs::metadata(&slot).unwrap().permissions().mode() & 0o777;
        assert_eq!(slot_mode, 0o600, "user.json must be mode 0600");
    }
}

#[test]
fn login_rejects_token_argument() {
    for arguments in [
        vec!["login", "--token", "jz_must_not_enter_argv"],
        vec!["login", "--token=jz_must_not_enter_argv"],
    ] {
        let home = tempfile::tempdir().expect("tempdir");
        easybooks()
            .env("HOME", home.path())
            .args(arguments)
            .assert()
            .failure()
            .stdout(predicate::str::contains("jz_must_not_enter_argv").not())
            .stderr(predicate::str::contains("jz_must_not_enter_argv").not());
    }
}

#[test]
fn login_rejects_empty_or_multiline_stdin() {
    for input in [
        "\n",
        "jz_first\njz_second\n",
        "jz_extra_blank\n\n",
        " jz_leading_space\n",
    ] {
        let home = tempfile::tempdir().expect("tempdir");
        easybooks()
            .env("HOME", home.path())
            .args(["login", "--token-stdin"])
            .write_stdin(input)
            .assert()
            .failure()
            .stdout(predicate::str::contains("jz_").not())
            .stderr(predicate::str::contains("jz_").not());
        assert!(!home.path().join(".easybooks/config.json").exists());
    }
}

#[cfg(unix)]
#[test]
fn login_rejects_symlinked_config_directory() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().expect("home");
    let outside = tempfile::tempdir().expect("outside");
    symlink(outside.path(), home.path().join(".easybooks")).expect("create symlink");

    easybooks()
        .env("HOME", home.path())
        .args(["login", "--token-stdin"])
        .write_stdin("jz_symlink_test\n")
        .assert()
        .failure()
        .stdout(predicate::str::contains("jz_symlink_test").not())
        .stderr(predicate::str::contains("jz_symlink_test").not());

    assert!(!outside.path().join("config.json").exists());
}

#[cfg(unix)]
#[test]
fn login_and_load_reject_symlinked_config_file() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().expect("home");
    let config_directory = home.path().join(".easybooks");
    std::fs::create_dir(&config_directory).expect("create config directory");
    let outside = home.path().join("outside-config.json");
    std::fs::write(&outside, "must remain unchanged").expect("write outside target");
    symlink(&outside, config_directory.join("config.json")).expect("create config symlink");

    easybooks()
        .env("HOME", home.path())
        .args(["login", "--token-stdin"])
        .write_stdin("jz_symlink_file\n")
        .assert()
        .failure()
        .stdout(predicate::str::contains("jz_symlink_file").not())
        .stderr(predicate::str::contains("jz_symlink_file").not());
    easybooks()
        .env("HOME", home.path())
        .arg("whoami")
        .assert()
        .failure();

    assert_eq!(
        std::fs::read_to_string(outside).expect("read outside target"),
        "must remain unchanged"
    );
}

#[test]
fn login_rejects_oversized_stdin() {
    let home = tempfile::tempdir().expect("home");
    easybooks()
        .env("HOME", home.path())
        .args(["login", "--token-stdin"])
        .write_stdin("x".repeat(4097))
        .assert()
        .failure();
    assert!(!home.path().join(".easybooks/config.json").exists());
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
        .args(["login", "--token-stdin"])
        .write_stdin("jz_envonly\n")
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
    let arg_url = "http://192.168.1.69:8310";
    easybooks()
        .env("HOME", home.path())
        .env("EASYBOOKS_API_URL", TEST_URL)
        .args(["login", "--token-stdin", "--base-url", arg_url])
        .write_stdin("jz_argwins\n")
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
        .args(["login", "--token-stdin"])
        .write_stdin("jz_default\n")
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
            r#"{{"ok":true,"user_id":"{USER}","email":"user@example.com","scope":"read_write","source":"easybooks-integration"}}"#
        ))
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["whoami"])
        .assert()
        .success()
        .stdout(predicate::str::contains(USER))
        .stdout(predicate::str::contains(r#""email": "user@example.com""#))
        .stdout(predicate::str::contains(r#""scope": "read_write""#))
        .stdout(predicate::str::contains(r#""api_key_masked": "jz_***""#));

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

#[test]
fn tx_reclassify_posts_classification_and_learn_with_bearer() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/transactions/txn_77/reclassify")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{"classification":"mixed","learn":true}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"classification":"mixed","learned":true}"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "tx",
            "reclassify",
            "txn_77",
            "--class",
            "mixed",
            "--learn",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""classification": "mixed""#));

    mock.assert();
}

#[test]
fn tx_reclassify_defaults_learn_false() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/transactions/txn_77/reclassify")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{"classification":"personal","learn":false}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true}"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args(["tx", "reclassify", "txn_77", "--class", "personal"])
        .assert()
        .success();

    mock.assert();
}

#[test]
fn tx_attach_receipt_uploads_base64_and_prints_receipt_url() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("receipt.pdf");
    // "%PDF-1.4" → base64 "JVBERi0xLjQ=".
    std::fs::write(&file, b"%PDF-1.4").expect("write fixture");

    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/integrations/transactions/txn_88/receipt")
        .match_header("authorization", BEARER)
        .match_body(Matcher::JsonString(
            r#"{"filename":"receipt.pdf","content_type":"application/pdf","content_base64":"JVBERi0xLjQ="}"#
                .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"receipt_url":"https://cdn.example/receipts/txn_88.pdf"}"#)
        .create();

    easybooks()
        .env("EASYBOOKS_API_URL", server.url())
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "tx",
            "attach-receipt",
            "txn_88",
            "--file",
            file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://cdn.example/receipts/txn_88.pdf",
        ));

    mock.assert();
}

#[test]
fn tx_attach_receipt_refuses_oversize_file_locally() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("big.png");
    // 10 MiB + 1 byte → over the local ceiling; must fail before any network.
    std::fs::write(&file, vec![0u8; 10 * 1024 * 1024 + 1]).expect("write fixture");

    // Point at an unroutable URL; if the CLI tried to POST it would hang/fail,
    // but the size guard must short-circuit first with a clear message.
    easybooks()
        .env("EASYBOOKS_API_URL", "http://127.0.0.1:9")
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "tx",
            "attach-receipt",
            "txn_88",
            "--file",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("10 MB limit"));
}

#[test]
fn tx_attach_receipt_rejects_unsupported_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"hello").expect("write fixture");

    easybooks()
        .env("EASYBOOKS_API_URL", "http://127.0.0.1:9")
        .env("EASYBOOKS_API_KEY", KEY)
        .args([
            "tx",
            "attach-receipt",
            "txn_88",
            "--file",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported receipt type"));
}
