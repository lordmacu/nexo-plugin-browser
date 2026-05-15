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

use serde::{Deserialize, Serialize};

/// Operator YAML wire shape. The top-level `browser:` key accepts either
/// a single map (legacy 0.2.x single-instance) or a sequence of maps
/// (0.3.0+ multi-instance). The plugin normalises both to
/// `Vec<BrowserConfig>` via [`BrowserPluginShape::into_vec`].
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BrowserPluginConfigFile {
    pub browser: BrowserPluginShape,
}

/// 0.2.x compatibility alias. Existing operator YAML keeps working;
/// new code should prefer [`BrowserPluginConfigFile`].
pub type BrowserConfigFile = BrowserPluginConfigFile;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum BrowserPluginShape {
    Single(BrowserConfig),
    Many(Vec<BrowserConfig>),
}

impl BrowserPluginShape {
    /// Flatten to the canonical `Vec<BrowserConfig>` the registry
    /// consumes. Single-map shape produces a 1-element vec so the
    /// downstream boot loop is shape-agnostic.
    pub fn into_vec(self) -> Vec<BrowserConfig> {
        match self {
            Self::Single(c) => vec![c],
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    /// Instance label. `None` ⇒ legacy single-instance behaviour
    /// (resolved to `"default"` at boot time).
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub executable: String,
    /// Empty = launch new Chrome. Set to e.g. "http://127.0.0.1:9222" to attach.
    #[serde(default)]
    pub cdp_url: String,
    /// Empty ⇒ daemon resolves to `${state_dir}/instances/<resolved_label>/`
    /// at boot time. An explicit non-empty value overrides the auto-resolved
    /// path so operators can pin a profile to a shared filesystem location.
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
    /// Agents permitted to route browser tools to this instance. Empty
    /// = accept any agent (back-compat with 0.2.x).
    #[serde(default)]
    pub allow_agents: Vec<String>,
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
