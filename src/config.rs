//! Phase 93.4.e — plugin-owned config types.
//!
//! Until 0.2.4 this plugin re-exported `nexo_config::BrowserConfig`.
//! Phase 93 inverts: each plugin owns its config contract
//! (manifest's `[plugin.config_schema]` + this module's Rust
//! definitions); the daemon delivers the operator YAML opaquely
//! via `plugin.configure` JSON-RPC.
//!
//! Field shapes mirror `nexo_config::types::plugins::BrowserConfig`
//! verbatim — operator YAML keeps working unchanged.

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfigFile {
    pub browser: BrowserConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub executable: String,
    /// Empty = launch new Chrome. Set to e.g. "http://127.0.0.1:9222" to attach.
    #[serde(default)]
    pub cdp_url: String,
    #[serde(default = "default_user_data_dir")]
    pub user_data_dir: String,
    #[serde(default = "default_window_width")]
    pub window_width: u32,
    #[serde(default = "default_window_height")]
    pub window_height: u32,
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_command_timeout_ms")]
    pub command_timeout_ms: u64,
    /// Extra CLI flags forwarded verbatim to the spawned Chrome/Chromium
    /// process.
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_user_data_dir() -> String {
    "./data/browser/profile".to_string()
}
fn default_window_width() -> u32 {
    1280
}
fn default_window_height() -> u32 {
    800
}
fn default_connect_timeout_ms() -> u64 {
    10_000
}
fn default_command_timeout_ms() -> u64 {
    15_000
}
