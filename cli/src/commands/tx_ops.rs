//! Per-transaction operations (contract §2): reclassify an existing transaction
//! and attach a receipt document to it.
//!
//!   - `tx reclassify <id> --class <business|mixed|personal> [--learn]`
//!       → POST /api/integrations/transactions/{id}/reclassify
//!   - `tx attach-receipt <id> --file <path>`
//!       → POST /api/integrations/transactions/{id}/receipt
//!
//! Both go through the bundled `ApiClient` so they carry the user's `eb_live_`
//! Bearer key exactly like every other write. Identity comes from the key, so
//! no owner id is sent in the body or path.

use crate::client::ApiClient;
use crate::output;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use std::path::Path;

/// Hard local ceiling on a receipt upload before we even open a socket. The
/// backend may impose a smaller limit; this just refuses the obviously-too-big
/// file with a clear, offline error instead of a slow failed round-trip.
const MAX_RECEIPT_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

/// `easybooks tx reclassify <transaction_id> --class <business|mixed|personal> [--learn]`
///
/// Correct the deductibility classification of an already-recorded transaction.
/// `--learn` asks the backend to remember the correction for this sender so
/// future transactions from the same source are classified the same way.
pub fn reclassify(
    client: &ApiClient,
    transaction_id: &str,
    class: &str,
    learn: bool,
) -> Result<()> {
    if transaction_id.trim().is_empty() {
        return Err(anyhow!("transaction_id is required"));
    }
    validate_reclass(class)?;

    let body = json!({ "classification": class, "learn": learn });
    let path = format!(
        "/api/integrations/transactions/{}/reclassify",
        encode_segment(transaction_id)
    );
    let resp = client.post(&path, &body)?;
    output::print_json(&resp)
}

/// `easybooks tx attach-receipt <transaction_id> --file <path>`
///
/// Read the local file, guess its content_type from the extension, base64-encode
/// it, and attach it to the transaction. Files larger than 10 MiB are refused
/// locally with a clear error. On success the backend's `receipt_url` is printed.
pub fn attach_receipt(client: &ApiClient, transaction_id: &str, file: &str) -> Result<()> {
    if transaction_id.trim().is_empty() {
        return Err(anyhow!("transaction_id is required"));
    }

    let path = Path::new(file);
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("cannot read receipt file: {}", file))?;
    if !metadata.is_file() {
        return Err(anyhow!("not a regular file: {}", file));
    }
    if metadata.len() > MAX_RECEIPT_BYTES {
        return Err(anyhow!(
            "receipt file is {:.2} MB which exceeds the 10 MB limit — compress it or attach a smaller copy ({})",
            metadata.len() as f64 / (1024.0 * 1024.0),
            file
        ));
    }

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("could not determine a filename from path: {}", file))?
        .to_string();

    let content_type = guess_content_type(&filename).ok_or_else(|| {
        anyhow!(
            "unsupported receipt type for {:?} — allowed: png, jpg/jpeg, gif, webp, heic, heif, pdf",
            filename
        )
    })?;

    let bytes = std::fs::read(path).with_context(|| format!("reading receipt file: {}", file))?;
    let content_base64 = BASE64.encode(&bytes);

    let body = json!({
        "filename": filename,
        "content_type": content_type,
        "content_base64": content_base64,
    });
    let api_path = format!(
        "/api/integrations/transactions/{}/receipt",
        encode_segment(transaction_id)
    );
    let resp = client.post(&api_path, &body)?;

    // Surface the receipt_url prominently when the backend returns one; always
    // echo the full response so nothing is hidden.
    let receipt_url = resp
        .get("receipt_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    match receipt_url {
        Some(url) => output::print_json(&json!({ "receipt_url": url, "response": resp })),
        None => output::print_json(&resp),
    }
}

/// Validate the reclassify label is one of the three deductibility classes.
fn validate_reclass(class: &str) -> Result<()> {
    match class {
        "business" | "mixed" | "personal" => Ok(()),
        _ => Err(anyhow!(
            "--class must be one of business|mixed|personal (got {:?})",
            class
        )),
    }
}

/// Map a filename extension to the receipt content_type the backend accepts.
/// Returns `None` for unsupported extensions so the caller errors clearly
/// instead of guessing a wrong type.
pub fn guess_content_type(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next().map(|e| e.to_ascii_lowercase())?;
    let ct = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "pdf" => "application/pdf",
        _ => return None,
    };
    Some(ct)
}

/// Percent-encode the characters that would break a path segment. The
/// transaction id is normally a uuid, but we stay defensive.
fn encode_segment(value: &str) -> String {
    value.replace('/', "%2F").replace(' ', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_by_extension() {
        assert_eq!(guess_content_type("a.png"), Some("image/png"));
        assert_eq!(guess_content_type("a.PNG"), Some("image/png"));
        assert_eq!(guess_content_type("scan.jpg"), Some("image/jpeg"));
        assert_eq!(guess_content_type("scan.jpeg"), Some("image/jpeg"));
        assert_eq!(guess_content_type("anim.gif"), Some("image/gif"));
        assert_eq!(guess_content_type("pic.webp"), Some("image/webp"));
        assert_eq!(guess_content_type("photo.heic"), Some("image/heic"));
        assert_eq!(guess_content_type("photo.heif"), Some("image/heif"));
        assert_eq!(guess_content_type("invoice.pdf"), Some("application/pdf"));
        assert_eq!(guess_content_type("notes.txt"), None);
        assert_eq!(guess_content_type("noextension"), None);
    }

    #[test]
    fn reclass_validation() {
        assert!(validate_reclass("business").is_ok());
        assert!(validate_reclass("mixed").is_ok());
        assert!(validate_reclass("personal").is_ok());
        assert!(validate_reclass("Business").is_err());
        assert!(validate_reclass("deductible").is_err());
    }
}
