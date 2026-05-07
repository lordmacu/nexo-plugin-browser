//! Subprocess-side BrowserPlugin: launches Chrome, attaches a CDP
//! session, dispatches [`BrowserCmd`] → [`BrowserResult`].
//!
//! Phase 81.17.c.crates-publish slim-down — the in-tree daemon
//! integration (broker bridge, circuit breaker, `Plugin` /
//! `NexoPlugin` trait impls, manifest parsing) was removed
//! because the subprocess flow handles those concerns at the
//! `nexo_microapp_sdk::plugin::PluginAdapter` layer:
//!
//! - **Broker bridge**: PluginAdapter publishes/consumes broker
//!   events through the daemon-routed JSON-RPC stdio frames.
//! - **Manifest**: `include_str!("../nexo-plugin.toml")` is
//!   parsed once by PluginAdapter::new — no need for a second
//!   `cached_manifest`.
//! - **Plugin trait**: irrelevant for subprocess plugins; the
//!   daemon's `SubprocessNexoPlugin` adapter speaks the wire
//!   protocol on the host side.
//! - **CircuitBreaker**: replaced by per-command timeouts;
//!   subprocess crashes / supervisor logic live in
//!   `nexo-core::nexo_plugin_registry` (Phase 81.21).
//!
//! What remains is the minimal CDP driver — enough for
//! `dispatch.rs` to translate every `tool.invoke` into a
//! `BrowserCmd` and call `execute`.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use tokio::sync::Mutex;

use nexo_cdp::{CdpClient, CdpSession};
use nexo_config::BrowserConfig;

use crate::chrome::{ChromeLauncher, RunningChrome};
use crate::command::{BrowserCmd, BrowserResult};

/// Single-Chrome-instance state shared across the subprocess
/// lifetime. `Arc<Mutex<...>>` so the caller can cheaply clone
/// references into futures.
pub struct BrowserPlugin {
    inner: Arc<BrowserInner>,
}

struct BrowserInner {
    config: BrowserConfig,
    session: Mutex<Option<CdpSession>>,
    chrome: Mutex<Option<RunningChrome>>,
}

impl BrowserPlugin {
    pub fn new(config: BrowserConfig) -> Self {
        Self {
            inner: Arc::new(BrowserInner {
                config,
                session: Mutex::new(None),
                chrome: Mutex::new(None),
            }),
        }
    }

    /// Execute one CDP command. Reuses the existing
    /// session-or-launch flow; lazy-boots Chrome on the first
    /// call. Per-command timeout from
    /// `BrowserConfig.command_timeout_ms`.
    pub async fn execute(&self, cmd: BrowserCmd) -> BrowserResult {
        if let Err(e) = self.inner.ensure_session().await {
            return BrowserResult::err(format!("session init failed: {e}"));
        }
        let mut lock = self.inner.session.lock().await;
        let Some(session) = lock.as_mut() else {
            return BrowserResult::err("no active session");
        };
        let timeout = Duration::from_millis(self.inner.config.command_timeout_ms);
        match cmd {
            BrowserCmd::Navigate { url } => {
                match tokio::time::timeout(timeout, session.navigate(&url)).await {
                    Ok(Ok(())) => BrowserResult::ok(),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("navigate timed out"),
                }
            }
            BrowserCmd::Click { target } => {
                match tokio::time::timeout(timeout, session.click(&target)).await {
                    Ok(Ok(())) => BrowserResult::ok(),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("click timed out"),
                }
            }
            BrowserCmd::Fill { target, value } => {
                match tokio::time::timeout(timeout, session.fill(&target, &value)).await {
                    Ok(Ok(())) => BrowserResult::ok(),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("fill timed out"),
                }
            }
            BrowserCmd::Screenshot => {
                match tokio::time::timeout(timeout, session.screenshot()).await {
                    Ok(Ok(bytes)) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        BrowserResult::ok_screenshot(b64)
                    }
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("screenshot timed out"),
                }
            }
            BrowserCmd::Evaluate { script } => {
                match tokio::time::timeout(timeout, session.evaluate(&script)).await {
                    Ok(Ok(v)) => BrowserResult::ok_value(v),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("evaluate timed out"),
                }
            }
            BrowserCmd::Snapshot => {
                match tokio::time::timeout(timeout, session.snapshot()).await {
                    Ok(Ok(text)) => BrowserResult::ok_snapshot(text),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("snapshot timed out"),
                }
            }
            BrowserCmd::ScrollTo { target } => {
                match tokio::time::timeout(timeout, session.scroll_to(&target)).await {
                    Ok(Ok(())) => BrowserResult::ok(),
                    Ok(Err(e)) => BrowserResult::err(e.to_string()),
                    Err(_) => BrowserResult::err("scroll_to timed out"),
                }
            }
        }
    }
}

impl BrowserPlugin {
    /// Phase 81.17.c.multi-profile — close the active Chrome
    /// session + kill the spawned Chrome process. Idempotent: a
    /// second call after the inner state is already empty is a
    /// no-op.
    ///
    /// Drop order matters: clear `session` first so any pending
    /// CDP commands fail with the existing "no active session"
    /// branch, then clear `chrome` (whose `RunningChrome::Drop`
    /// kills the OS process). Both fields stay re-bootable —
    /// the next [`Self::execute`] call will lazy-reboot a fresh
    /// Chrome with the same `user_data_dir` so cookies / login
    /// state persist across eviction cycles.
    pub async fn shutdown_chrome(&self) {
        let mut session = self.inner.session.lock().await;
        *session = None;
        drop(session);
        let mut chrome = self.inner.chrome.lock().await;
        *chrome = None;
    }
}

impl BrowserInner {
    async fn ensure_session(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut session_lock = self.session.lock().await;
        if session_lock.is_some() {
            return Ok(());
        }

        let ws_url = if self.config.cdp_url.is_empty() {
            let chrome = ChromeLauncher::launch(&self.config).await?;
            let ws = chrome.ws_url.clone();
            *self.chrome.lock().await = Some(chrome);
            ws
        } else {
            ChromeLauncher::connect_existing(&self.config.cdp_url, self.config.connect_timeout_ms)
                .await?
        };

        let client = Arc::new(CdpClient::connect(&ws_url).await?);

        if let Err(e) = client.send("Page.enable", serde_json::json!({})).await {
            tracing::warn!(error = %e, "Page.enable failed — navigation events may be missing");
        }

        let target_id = get_first_target_id(&ws_url, self.config.connect_timeout_ms).await?;
        let session = CdpSession::new(
            Arc::clone(&client),
            &target_id,
            self.config.command_timeout_ms,
        )
        .await?;
        *session_lock = Some(session);
        Ok(())
    }
}

async fn get_first_target_id(ws_url: &str, timeout_ms: u64) -> anyhow::Result<String> {
    let http_url = ws_url
        .replace("ws://", "http://")
        .replace("wss://", "https://");
    let base = if let Some(idx) = http_url.find("/devtools/") {
        http_url[..idx].to_string()
    } else {
        http_url
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()?;
    let targets: Vec<serde_json::Value> = client
        .get(format!("{base}/json/list"))
        .header(reqwest::header::HOST, "localhost")
        .send()
        .await?
        .json()
        .await?;

    targets
        .iter()
        .find(|t| t.get("type").and_then(|v| v.as_str()) == Some("page"))
        .and_then(|t| t.get("id").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("no page target found in Chrome"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BrowserConfig {
        BrowserConfig {
            headless: true,
            executable: String::new(),
            cdp_url: String::new(),
            user_data_dir: "./.test-profile".into(),
            window_width: 1280,
            window_height: 800,
            connect_timeout_ms: 8_000,
            command_timeout_ms: 30_000,
            args: Vec::new(),
        }
    }

    #[tokio::test]
    async fn shutdown_chrome_is_idempotent_on_fresh_plugin() {
        // BrowserPlugin::new doesn't boot Chrome (lazy); session
        // and chrome are both `None`. shutdown_chrome must be a
        // no-op (no panic on locking-empty-Mutex, no error).
        let plugin = BrowserPlugin::new(test_config());
        plugin.shutdown_chrome().await;
        plugin.shutdown_chrome().await; // second call must also no-op.
    }
}

