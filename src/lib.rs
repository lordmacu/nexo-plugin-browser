//! Browser plugin library — copied verbatim from
//! `proyecto/crates/plugins/browser/src/` so the standalone
//! binary reuses the existing CDP client + Chrome launcher +
//! tool dispatch logic.
//!
//! `main.rs` is the new subprocess entrypoint that wraps a
//! [`BrowserPlugin`] in the nexo-microapp-sdk
//! [`PluginAdapter`](nexo_microapp_sdk::plugin::PluginAdapter) and
//! routes incoming `tool.invoke` requests through
//! [`BrowserPlugin::execute`].

// CDP client lives in workspace crate `nexo-cdp`
// (Phase 81.17.c follow-up `nexo-cdp-extract`).
pub use nexo_cdp as cdp;
pub mod chrome;
pub mod command;
pub mod dispatch;
pub mod env_config;
pub mod plugin;
pub mod tool_defs;

pub use chrome::{ChromeLauncher, RunningChrome};
pub use command::{BrowserCmd, BrowserResult};
pub use nexo_cdp::{CdpClient, CdpSession};
pub use plugin::BrowserPlugin;
pub use tool_defs::browser_tool_defs;
