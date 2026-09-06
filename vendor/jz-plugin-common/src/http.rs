//! Thin ureq wrapper — every non-2xx is an error; body excerpts redact secrets.

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use ureq::{Agent, AgentBuilder};

const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("HTTP {code}: {body_excerpt}")]
    Status { code: u16, body_excerpt: String },
    #[error("network error: {0}")]
    Transport(String),
    #[error("response decode failed: {0}")]
    Decode(String),
}

#[derive(Debug, Clone)]
pub struct Response<T> {
    pub status: u16,
    pub body: T,
}

pub fn agent() -> Agent {
    AgentBuilder::new().redirects(0).build()
}

pub fn send_json<T, B>(
    method: &str,
    url: &str,
    bearer: Option<&str>,
    body: Option<&B>,
) -> Result<Response<T>, HttpError>
where
    T: DeserializeOwned,
    B: Serialize,
{
    let mut request = agent().request(method, url);
    request = request
        .set("Accept", "application/json")
        .set("Accept-Encoding", "identity");
    if let Some(token) = bearer {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    let response = match body {
        Some(value) => request
            .set("Content-Type", "application/json")
            .send_json(value),
        None => request.call(),
    }
    .map_err(map_ureq_error)?;
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(HttpError::Status {
            code: status,
            body_excerpt: redact_body_excerpt(&text),
        });
    }
    let parsed: T = serde_json::from_str(&text).map_err(|error| {
        HttpError::Decode(format!(
            "JSON decode: {error}; excerpt={}",
            redact_body_excerpt(&text)
        ))
    })?;
    Ok(Response {
        status,
        body: parsed,
    })
}

pub fn send_json_value(
    method: &str,
    url: &str,
    bearer: Option<&str>,
    body: Option<&serde_json::Value>,
) -> Result<Response<serde_json::Value>, HttpError> {
    send_json(method, url, bearer, body)
}

pub fn post_json_value(
    url: &str,
    bearer: Option<&str>,
    body: &serde_json::Value,
) -> Result<Response<serde_json::Value>, HttpError> {
    send_json_value("POST", url, bearer, Some(body))
}

fn map_ureq_error(error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::Status(code, response) => {
            let text = response.into_string().unwrap_or_default();
            HttpError::Status {
                code,
                body_excerpt: redact_body_excerpt(&text),
            }
        }
        ureq::Error::Transport(transport) => {
            HttpError::Transport(format!("{:?}", transport.kind()))
        }
    }
}

pub fn redact_body_excerpt(text: &str) -> String {
    let mut out = text.chars().take(512).collect::<String>();
    if text.chars().count() > 512 {
        out.push('…');
    }
    redact_secrets(&out)
}

/// Shared §4 scanner: secrets/tokens in envelope-facing text.
pub fn contains_forbidden_secret(text: &str) -> bool {
    if text.contains("jz_") || text.contains("Bearer ") {
        return true;
    }
    if text.contains("eyJ") {
        return true;
    }
    contains_jwt_shape(text)
}

pub fn looks_like_absolute_path(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("\\\\")
        || trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && trimmed.get(1..=2) == Some(":\\")
}

pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    while let Some(start) = out.find("Bearer ") {
        let rest = &out[start + 7..];
        if rest.starts_with(REDACTED) {
            break;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '"')
            .unwrap_or(rest.len());
        if end == 0 {
            break;
        }
        out.replace_range(start..start + 7 + end, "Bearer [REDACTED]");
    }
    for prefix in ["jz_", "eyJ"] {
        while let Some(idx) = out.find(prefix) {
            let tail = &out[idx..];
            let len = tail
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'' && *c != ',')
                .count();
            if len == 0 {
                break;
            }
            out.replace_range(idx..idx + len, REDACTED);
        }
    }
    redact_jwt_shapes(&mut out);
    out
}

fn redact_jwt_shapes(out: &mut String) {
    loop {
        let chars: Vec<char> = out.chars().collect();
        let mut replaced = false;
        for index in 0..chars.len() {
            if let Some(len) = jwt_token_len(&chars[index..]) {
                let token: String = chars[index..index + len].iter().collect();
                if !token.contains(REDACTED) {
                    out.replace_range(index..index + len, REDACTED);
                    replaced = true;
                    break;
                }
            }
        }
        if !replaced {
            break;
        }
    }
}

fn jwt_token_len(chars: &[char]) -> Option<usize> {
    let mut dots = 0;
    let mut length = 0;
    for &ch in chars {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            length += 1;
            continue;
        }
        if ch == '.' && dots < 2 {
            dots += 1;
            length += 1;
            continue;
        }
        break;
    }
    if dots == 2 && length > 6 {
        Some(length)
    } else {
        None
    }
}

fn contains_jwt_shape(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|index| jwt_token_len(&chars[index..]).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_jz_token_in_body_excerpt() {
        let body = r#"{"error":"bad","token":"jz_abc123secret"}"#;
        let excerpt = redact_body_excerpt(body);
        assert!(!excerpt.contains("jz_abc123secret"));
        assert!(excerpt.contains(REDACTED));
    }

    #[test]
    fn http_500_redacts_token_in_error() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/fail")
            .with_status(500)
            .with_body(r#"{"detail":"refused jz_supersecret_token"}"#)
            .create();
        let url = format!("{}/fail", server.url());
        let err = send_json_value("GET", &url, None, None).unwrap_err();
        mock.assert();
        match err {
            HttpError::Status { body_excerpt, .. } => {
                assert!(!body_excerpt.contains("jz_supersecret_token"));
            }
            other => panic!("expected status error, got {other:?}"),
        }
    }

    #[test]
    fn redacts_bearer_and_jwt_shapes_from_display() {
        let jwt = "abc.def.ghi";
        let body = format!(r#"{{"authorization":"Bearer eyJsecret.payload.sig","other":"{jwt}"}}"#);
        let err = HttpError::Status {
            code: 500,
            body_excerpt: redact_body_excerpt(&body),
        };
        let rendered = err.to_string();
        assert!(!rendered.contains("eyJsecret"));
        assert!(!rendered.contains("Bearer eyJ"));
        assert!(!rendered.contains("abc.def.ghi"));
        if let HttpError::Status { body_excerpt, .. } = err {
            assert!(!body_excerpt.contains("eyJsecret"));
            assert!(!body_excerpt.contains("abc.def.ghi"));
        }
    }

    #[test]
    fn and_or_is_not_treated_as_path() {
        assert!(!looks_like_absolute_path("Choose Continue and/or Cancel."));
    }
}
