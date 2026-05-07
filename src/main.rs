//! Subprocess entrypoint for `nexo-plugin-browser` (Phase 81.17.c).
//!
//! Wires:
//!   - [`PluginAdapter`] — child-side JSON-RPC dispatch loop.
//!   - [`browser_tool_defs`] — 12 `browser_*` tool defs advertised
//!     in the initialize reply.
//!   - [`dispatch_browser_tool`] — single dispatcher routing
//!     `tool.invoke` requests to a lazily-booted shared
//!     [`BrowserPlugin`] (one Chrome instance per subprocess
//!     lifetime).
//!
//! Configuration flows from the daemon via env vars set by
//! `proyecto/src/main.rs::seed_browser_subprocess_env` —
//! `NEXO_PLUGIN_BROWSER_HEADLESS`,
//! `NEXO_PLUGIN_BROWSER_USER_DATA_DIR`,
//! `NEXO_PLUGIN_BROWSER_CDP_URL`, etc.

use std::sync::Arc;

use nexo_microapp_sdk::plugin::{PluginAdapter, ToolInvocation};
use tokio::sync::OnceCell;

use nexo_plugin_browser::{
    browser_tool_defs, dispatch::dispatch_browser_tool, env_config::browser_config_from_env,
    BrowserPlugin,
};

const MANIFEST: &str = include_str!("../nexo-plugin.toml");

/// Global lazily-initialised BrowserPlugin. First `tool.invoke`
/// boots Chrome (or attaches to an existing CDP); subsequent calls
/// reuse the same session. Lifetime = process lifetime.
static PLUGIN: OnceCell<Arc<BrowserPlugin>> = OnceCell::const_new();

async fn shared_plugin() -> Arc<BrowserPlugin> {
    PLUGIN
        .get_or_init(|| async {
            let cfg = browser_config_from_env();
            tracing::info!(
                headless = cfg.headless,
                cdp_url = %cfg.cdp_url,
                user_data_dir = %cfg.user_data_dir,
                "browser-plugin: lazy-initialising BrowserPlugin from env"
            );
            Arc::new(BrowserPlugin::new(cfg))
        })
        .await
        .clone()
}

#[tokio::main]
async fn main() -> nexo_microapp_sdk::Result<()> {
    nexo_microapp_sdk::init_logging_from_env("nexo-plugin-browser");

    PluginAdapter::new(MANIFEST)?
        .declare_tools(browser_tool_defs())
        .on_tool(|inv: ToolInvocation| async move {
            let plugin = shared_plugin().await;
            dispatch_browser_tool(plugin, &inv.tool_name, inv.args).await
        })
        .run_stdio()
        .await
}
