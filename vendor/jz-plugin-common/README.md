# jz-plugin-common

Shared Rust crate for Jacky official Portal plugin CLIs: unified auth exchange, HTTP helpers, Tell-Jacky, Product Signals, typed readiness envelopes, platform home paths, and common doctor sections.

## Consumption

Add a **pinned git rev** dependency (no branch refs):

```toml
[dependencies]
jz-plugin-common = { git = "ssh://git@github.com/jackyzhang69/platform.git", rev = "<pinned-sha>" }
```

Construct one compile-time [`PluginIdentity`](src/identity.rs) per plugin (`plugin_id`, optional `aud`, `auth_mode`, `product_scopes`). Call module APIs with that identity and the accountd base URL.

## Modules

| Module | Role |
|--------|------|
| `identity` | Per-plugin constants |
| `home` | `~/.jackyzhang.app` layout; `$JACKYZHANG_APP_HOME` override only |
| `http` | ureq JSON wrapper; non-2xx → typed error; redacted excerpts |
| `auth` | Durable slot read; exchange + in-memory JWT cache; single 401 retry |
| `tell_jacky` | Submit feedback; always mirror locally (`accountd` or `local_mirror`) |
| `signals` | Batch event ingest; returns error without aborting caller logic |
| `envelope` | `jz.plugin.envelope.v1` builders + validation |
| `human_action_ledger` | `jz.plugin.human_action_ledger.v1` trace validation |
| `doctor` | Embeddable credential/install section JSON |

## Migration rule

When adopting this crate, **delete** the plugin’s local copy of the same capability. No wrapper layers, no fallback to the old implementation, no dual-write. One authoritative path per capability.

## No silent success

- HTTP: every non-2xx is `HttpError::Status`; transport/decode failures are errors, never `Ok`.
- Tell-Jacky: `Delivered` only after 2xx **and** a parsed server `id`; otherwise `LocalMirrorOnly` with reason.
- Signals: ingest failure returns `SignalsError`; the caller decides whether to ignore (fail-open product rule).

JWT exchange validates shape, `aud`, `exp`, and allowed `alg` on the TLS-protected accountd response. **Signature verification is not performed** in this client library.
