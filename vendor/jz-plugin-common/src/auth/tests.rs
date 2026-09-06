use super::{
    clear_exchange_cache_for_tests, exchange, patch_cached_refresh_at_for_tests, post_product_json,
    read_durable_token, AuthError, ProductJwt,
};
use crate::home;
use crate::http::HttpError;
use crate::identity::{AuthMode, PluginIdentity};
use crate::tell_jacky::{submit, FeedbackDraft, FeedbackOutcome, FeedbackType};
use crate::test_env;
use base64::Engine as _;
use serde_json::json;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IDENTITY: PluginIdentity = PluginIdentity {
    plugin_id: "anychat",
    aud: Some("anychat"),
    auth_mode: AuthMode::Exchange,
    product_scopes: &["read", "write"],
};

const ANYDOC: PluginIdentity = PluginIdentity {
    plugin_id: "anydoc",
    aud: Some("anydoc"),
    auth_mode: AuthMode::Exchange,
    product_scopes: &["read", "write"],
};

fn make_test_jwt(aud: &str, issuer: &str, exp_offset_secs: i64) -> String {
    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","typ":"JWT"}"#);
    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        + exp_offset_secs as f64;
    let payload = format!(r#"{{"aud":"{aud}","iss":"{issuer}","exp":{exp}}}"#);
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    format!("{header}.{body}.signature")
}

#[test]
fn exchange_success() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let jwt = make_test_jwt("anychat", &issuer, 120);
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
            .create();
        let product = exchange(&IDENTITY, &issuer).unwrap();
        mock.assert();
        assert_eq!(product.token, jwt);
    });
}

#[test]
fn exchange_401_retries_once_then_unauthorized() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(401)
            .with_body(r#"{"error":"invalid"}"#)
            .expect(2)
            .create();
        let err = exchange(&IDENTITY, &issuer).unwrap_err();
        mock.assert();
        assert!(matches!(err, AuthError::Unauthorized));
    });
}

#[test]
fn wrong_audience_in_jwt() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let jwt = make_test_jwt("wrong_aud", &issuer, 120);
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
            .create();
        let err = exchange(&IDENTITY, &issuer).unwrap_err();
        mock.assert();
        assert!(matches!(err, AuthError::WrongAudience));
    });
}

#[test]
fn exchange_503_is_http_status() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(503)
            .with_body("busy")
            .create();
        let err = exchange(&IDENTITY, &issuer).unwrap_err();
        mock.assert();
        match err {
            AuthError::Http(HttpError::Status { code, .. }) => assert_eq!(code, 503),
            other => panic!("expected http status, got {other:?}"),
        }
    });
}

#[test]
fn malformed_jwt() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(r#"{"access_token":"not.a.jwt"}"#)
            .create();
        let err = exchange(&IDENTITY, &issuer).unwrap_err();
        mock.assert();
        assert!(matches!(err, AuthError::Malformed));
    });
}

#[test]
fn expired_jwt() {
    test_env::with_home("jz_test_exchange", |_tmp, mut server| {
        let issuer = server.url();
        let jwt = make_test_jwt("anychat", &issuer, -30);
        let mock = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
            .create();
        let err = exchange(&IDENTITY, &issuer).unwrap_err();
        mock.assert();
        assert!(matches!(err, AuthError::Malformed));
    });
}

#[test]
fn exchange_tolerates_small_issuer_clock_lead_but_rejects_excessive_lifetime() {
    test_env::with_home("jz_clock_skew", |_tmp, mut server| {
        let issuer = server.url();
        for (remaining, accepted) in [(315, true), (360, false), (-1, false)] {
            clear_exchange_cache_for_tests();
            let jwt = make_test_jwt("anychat", &issuer, remaining);
            let mock = server
                .mock("POST", "/v1/token/exchange")
                .with_status(200)
                .with_body(format!(r#"{{"access_token":"{jwt}","expires_in":300}}"#))
                .expect(1)
                .create();
            let result = exchange(&IDENTITY, &issuer);
            assert_eq!(result.is_ok(), accepted, "remaining={remaining}");
            if !accepted {
                assert!(matches!(result, Err(AuthError::Malformed)));
            }
            mock.assert();
            mock.remove();
        }
    });
}

#[test]
fn not_connected_without_slot() {
    let _guard = test_env::lock();
    clear_exchange_cache_for_tests();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var(home::PLATFORM_HOME_ENV, tmp.path());
    let err = exchange(&IDENTITY, "http://127.0.0.1:9").unwrap_err();
    assert!(matches!(err, AuthError::NotConnected));
    std::env::remove_var(home::PLATFORM_HOME_ENV);
}

#[test]
fn short_ttl_jwt_is_not_served_from_cache_after_expiry() {
    test_env::with_home("jz_cache_ttl", |_tmp, mut server| {
        let issuer = server.url();
        // exp = now+40s with 30s skew → refresh_at ≈ +10s, so the entry is cached.
        let short = make_test_jwt("anychat", &issuer, 40);
        let fresh = make_test_jwt("anychat", &issuer, 120);
        let first_hit = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{short}"}}"#))
            .expect(1)
            .create();
        let first = exchange(&IDENTITY, &issuer).unwrap();
        assert_eq!(first.token, short);
        first_hit.assert();

        let cached = exchange(&IDENTITY, &issuer).unwrap();
        assert_eq!(cached.token, short);
        // mockito always serves the earliest matching mock; retire it so the
        // post-expiry exchange can only be satisfied by the fresh mock.
        first_hit.remove();
        let second_hit = server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{fresh}"}}"#))
            .expect(1)
            .create();

        let durable = read_durable_token()
            .unwrap()
            .expect("durable token in slot");
        patch_cached_refresh_at_for_tests(
            &issuer,
            &durable,
            "anychat",
            Instant::now() + Duration::from_secs(120),
        );
        thread::sleep(Duration::from_secs(41));

        let second = exchange(&IDENTITY, &issuer).unwrap();
        second_hit.assert();
        assert_eq!(second.token, fresh);
        assert_ne!(second.token, short);
    });
}

#[test]
fn product_route_401_retries_once_then_fails() {
    test_env::with_home("jz_product_401", |_tmp, mut server| {
        let issuer = server.url();
        let jwt = make_test_jwt("anychat", &issuer, 120);
        server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
            .expect(2)
            .create();
        let product = server
            .mock("POST", "/v1/products/anychat/events:batch")
            .with_status(401)
            .with_body(r#"{"error":"expired"}"#)
            .expect(2)
            .create();
        let url = format!("{issuer}/v1/products/anychat/events:batch");
        let err = post_product_json(&IDENTITY, &issuer, &url, &json!({"events": []})).unwrap_err();
        product.assert();
        assert!(matches!(err, AuthError::Unauthorized));
    });
}

#[test]
fn tell_jacky_product_401_retries_once() {
    test_env::with_home("jz_tj_product_401", |_tmp, mut server| {
        let issuer = server.url();
        let jwt = make_test_jwt("anydoc", &issuer, 120);
        server
            .mock("POST", "/v1/token/exchange")
            .with_status(200)
            .with_body(format!(r#"{{"access_token":"{jwt}"}}"#))
            .expect(2)
            .create();
        let feedback = server
            .mock("POST", "/v1/products/anydoc/feedback")
            .with_status(401)
            .with_body(r#"{"error":"expired"}"#)
            .expect(2)
            .create();
        let draft = FeedbackDraft {
            kind: FeedbackType::BugReport,
            title: "title".into(),
            description: "desc".into(),
            idempotency_key: "idem-401".into(),
            client_version: "0.1.0".into(),
            url: None,
            context: None,
        };
        let outcome = submit(&ANYDOC, &issuer, draft).unwrap();
        feedback.assert();
        assert!(matches!(outcome, FeedbackOutcome::LocalMirrorOnly { .. }));
    });
}

#[test]
fn product_jwt_debug_masks_token() {
    let jwt = ProductJwt {
        token: "eyJsecret.payload.sig".into(),
    };
    let rendered = format!("{jwt:?}");
    assert!(!rendered.contains("eyJsecret"));
    assert!(rendered.contains("[REDACTED]"));
}
