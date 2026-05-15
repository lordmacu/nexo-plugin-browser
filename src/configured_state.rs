//! Phase 93.4.e — operator config slice delivered via
//! `plugin.configure` JSON-RPC (Phase 93.2). Single-instance
//! plugin (like email; unlike telegram/whatsapp which are arrays).

use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::config::BrowserConfig;

static CONFIGURED: OnceLock<Arc<RwLock<Option<BrowserConfig>>>> = OnceLock::new();

/// Process-wide configured-state cell. Initialised on first access.
pub fn configured_state() -> &'static Arc<RwLock<Option<BrowserConfig>>> {
    CONFIGURED.get_or_init(|| Arc::new(RwLock::new(None)))
}
