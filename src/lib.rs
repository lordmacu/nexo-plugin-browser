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

// Phase 81.17.c follow-up `nexo-cdp-extract` — CDP client moved
// to workspace crate `nexo-cdp`. Re-exported as `crate::cdp` so
// existing callers (`crate::cdp::CdpClient`, etc.) keep working
// without touching every import.
pub use nexo_cdp as cdp;
pub mod chrome;
pub mod command;
pub mod dispatch;
pub mod env_config;
pub mod plugin;
pub mod tool;
pub mod tool_defs;

pub use nexo_cdp::{CdpClient, CdpSession};
pub use chrome::{ChromeLauncher, RunningChrome};
pub use command::{BrowserCmd, BrowserResult};
pub use plugin::BrowserPlugin;
pub use tool_defs::browser_tool_defs;
