//! Common doctor section for plugin `doctor --json` output.

use crate::home::{self, Home};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CommonDoctor {
    pub plugin_id: &'static str,
    pub version: &'static str,
}

impl CommonDoctor {
    pub fn new(plugin_id: &'static str, version: &'static str) -> Self {
        Self { plugin_id, version }
    }

    pub fn section(&self) -> Result<Value, String> {
        let home = Home::resolve().map_err(|error| error.to_string())?;
        let configured = home::read_durable_token()
            .map_err(|error| error.to_string())?
            .is_some();
        let install_home = home.plugin_install_root(self.plugin_id);
        Ok(json!({
            "credential": {
                "connected": if configured { "connected" } else { "not_connected" },
                "token_slot": "user.json",
            },
            "install_home": {
                "path": install_home,
                "present": install_home.join("runtime-manifest.json").is_file(),
            },
            "plugin_id": self.plugin_id,
            "version": self.version,
        }))
    }
}
