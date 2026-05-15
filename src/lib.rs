//! Browser plugin library: the CDP client, Chrome launcher, and
//! tool dispatch logic.
//!
//! `main.rs` is the subprocess entrypoint that wraps a
//! [`BrowserPlugin`] in the nexo-microapp-sdk
//! [`PluginAdapter`](nexo_microapp_sdk::plugin::PluginAdapter) and
//! routes incoming `tool.invoke` requests through
//! [`BrowserPlugin::execute`].

// CDP client lives in the workspace crate `nexo-cdp`.
pub use nexo_cdp as cdp;
pub mod chrome;
pub mod command;
pub mod config;
pub mod configured_state;
pub mod dispatch;
pub mod env_config;
pub mod plugin;
pub mod profile;
pub mod profile_decoration;
pub mod profile_limits;
pub mod tool_defs;

pub use chrome::{ChromeLauncher, RunningChrome};
pub use command::{BrowserCmd, BrowserResult};
pub use config::{BrowserConfig, BrowserConfigFile, BrowserPluginConfigFile, BrowserPluginShape};
pub use configured_state::configured_state;
pub use nexo_cdp::{CdpClient, CdpSession};
pub use plugin::BrowserPlugin;
pub use profile::{
    profile_color_argb, sanitize_agent_id, user_data_dir_for, ProfileIdError,
};
pub use tool_defs::browser_tool_defs;
