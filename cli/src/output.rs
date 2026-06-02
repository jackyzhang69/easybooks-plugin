use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::io::{self, Write};

/// Print a success value as pretty JSON on stdout (machine-first; one JSON
/// object per invocation). Mirrors formbro `output::print_json`.
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    writeln!(out)?;
    Ok(())
}

/// Last-resort error printer for any anyhow path that escapes a command
/// handler. Per contract §2, an error is a non-zero exit + a JSON
/// `{"error": "..."}` envelope on stderr. We keep the full chain available as
/// `chain` for debugging, but the canonical machine-readable field is `error`.
///
/// The integration key is never part of an error message we construct, and the
/// HTTP client never embeds it in error text — so there is nothing to mask
/// here, but we still guard by never echoing raw request bodies.
pub fn print_error(error: &anyhow::Error) {
    let chain: Vec<String> = error.chain().map(|c| c.to_string()).collect();
    let payload = json!({
        "error": error.to_string(),
        "chain": chain,
    });
    let _ = serde_json::to_writer_pretty(io::stderr(), &payload);
    let _ = writeln!(io::stderr());
}
