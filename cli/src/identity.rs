use jz_plugin_common::identity::{AuthMode, PluginIdentity};

pub const PLUGIN_IDENTITY: PluginIdentity = PluginIdentity {
    plugin_id: "easybooks",
    aud: Some("eb"),
    auth_mode: AuthMode::Exchange,
    product_scopes: &["read", "write"],
};
