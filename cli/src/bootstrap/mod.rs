//! Binary / version resolution (contract §0).
//!
//! Unlike formbro, EasyBooks has NO lazy runtime assets (no pdfjs / webform
//! worker) — the CLI is a single self-contained binary. This module therefore
//! only carries the resolver logic that every skill's §B binary-resolution
//! block depends on, plus the cache-status detection that `doctor` uses to
//! warn about a stale plugin cache.
pub mod resolve;

pub use resolve::{
    current_platform, detect_cache_status, redact_home, resolve_binary, CacheStatus,
};
