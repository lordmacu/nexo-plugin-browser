//! Operator config slice delivered via `plugin.configure` JSON-RPC.
//!
//! Step 4 of browser-multi-instance: the cell now holds a
//! `Vec<BrowserConfig>` so the array shape (0.3.0 multi-instance)
//! and the legacy single-map shape (0.2.x, normalised to a 1-element
//! vec by `BrowserPluginShape::into_vec`) flow through the same
//! readers downstream.

use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::config::BrowserConfig;

static CONFIGURED: OnceLock<Arc<RwLock<Option<Vec<BrowserConfig>>>>> = OnceLock::new();

/// Process-wide configured-state cell. Initialised on first access.
pub fn configured_state() -> &'static Arc<RwLock<Option<Vec<BrowserConfig>>>> {
    CONFIGURED.get_or_init(|| Arc::new(RwLock::new(None)))
}
