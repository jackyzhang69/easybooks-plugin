//! Per-plugin identity constants supplied by each consumer at compile time.

/// How the plugin consumes the durable platform user credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// `POST /v1/token/exchange` → short-lived product JWT.
    Exchange,
    /// Raw `jz_` sent to product API; backend introspects at accountd.
    Introspect,
}

/// Compile-time plugin identity. Consumers define one `const` instance.
#[derive(Debug, Clone, Copy)]
pub struct PluginIdentity {
    pub plugin_id: &'static str,
    /// Exchange audience (`anypdf`, `eb`, …). `None` for introspect-only plugins.
    pub aud: Option<&'static str>,
    pub auth_mode: AuthMode,
    /// Scopes requested at exchange time (product-specific).
    pub product_scopes: &'static [&'static str],
}

impl PluginIdentity {
    pub fn exchange_aud(&self) -> Option<&'static str> {
        match self.auth_mode {
            AuthMode::Exchange => self.aud,
            AuthMode::Introspect => None,
        }
    }
}
