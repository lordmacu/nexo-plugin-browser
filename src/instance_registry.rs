//! Operator-declared instance registry. Step 3 of browser-multi-instance.
//!
//! Maps each instance label (operator YAML `browser[].instance`) to its
//! `Arc<BrowserPlugin>` — one Chrome handle per declared instance.
//!
//! Mirrors the multi-instance pattern from
//! `nexo-rs-plugin-whatsapp/src/bot_registry.rs:28`:
//! `OnceLock<Arc<DashMap<String, ...>>>` — same `Arc<DashMap>` instance
//! shared across the boot loop (`on_configure`), the dispatch resolver
//! (`dispatch::resolve_plugin`, Step 5), and admin RPC handlers
//! (`admin::list_instances`, Step 7).
//!
//! This registry coexists with the legacy per-`agent_id` `PROFILES`
//! map in `main.rs` (Phase 81.17.c.multi-profile). When the registry
//! is empty, dispatch falls back to the legacy map; when populated,
//! it wins.

use std::sync::{Arc, OnceLock};

use dashmap::DashMap;

use crate::plugin::BrowserPlugin;

static REGISTRY: OnceLock<Arc<DashMap<String, Arc<BrowserPlugin>>>> = OnceLock::new();

fn map() -> Arc<DashMap<String, Arc<BrowserPlugin>>> {
    REGISTRY.get_or_init(|| Arc::new(DashMap::new())).clone()
}

/// Register `plugin` under `label`. Overwrites any prior entry —
/// callers that need diff-aware behaviour (the boot loop) compare
/// against [`entries`] before re-registering.
pub fn register(label: &str, plugin: Arc<BrowserPlugin>) {
    map().insert(label.to_string(), plugin);
}

/// Drop the entry for `label`. Returns the prior `Arc<BrowserPlugin>`
/// so the caller can `.shutdown_chrome().await` on the removed value.
pub fn unregister(label: &str) -> Option<Arc<BrowserPlugin>> {
    map().remove(label).map(|(_, v)| v)
}

/// Look up the live `Arc<BrowserPlugin>` for `label`.
pub fn lookup(label: &str) -> Option<Arc<BrowserPlugin>> {
    map().get(label).map(|v| v.clone())
}

/// Snapshot every `(label, plugin)` pair. Useful for the dispatcher's
/// "exactly-one-declared" compat shim + admin `list_instances`.
pub fn entries() -> Vec<(String, Arc<BrowserPlugin>)> {
    map()
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect()
}

/// Number of declared instances. O(1).
pub fn len() -> usize {
    map().len()
}

/// Drop every entry. Used by tests; production callers should diff
/// against [`entries`] and unregister individual labels so live
/// Chromes can be shut down cleanly.
pub fn clear() {
    map().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BrowserConfig;
    use serial_test::serial;

    fn dummy_plugin() -> Arc<BrowserPlugin> {
        Arc::new(BrowserPlugin::new(BrowserConfig {
            instance: None,
            headless: true,
            executable: String::new(),
            cdp_url: String::new(),
            user_data_dir: "./.test-registry".into(),
            window_width: 1280,
            window_height: 800,
            connect_timeout_ms: 8_000,
            command_timeout_ms: 30_000,
            args: Vec::new(),
            allow_agents: Vec::new(),
        }))
    }

    // The registry is a process-wide singleton (`OnceLock`); tests
    // serialise to avoid cross-talk poisoning the assertions.

    #[test]
    #[serial]
    fn register_then_lookup_returns_same_arc() {
        clear();
        let p = dummy_plugin();
        register("alpha", p.clone());
        let got = lookup("alpha").expect("registered");
        assert!(Arc::ptr_eq(&p, &got));
        clear();
    }

    #[test]
    #[serial]
    fn unregister_removes_entry_and_returns_prior() {
        clear();
        let p = dummy_plugin();
        register("beta", p.clone());
        let removed = unregister("beta").expect("entry existed");
        assert!(Arc::ptr_eq(&p, &removed));
        assert!(lookup("beta").is_none());
        clear();
    }

    #[test]
    #[serial]
    fn unregister_unknown_returns_none() {
        clear();
        assert!(unregister("ghost").is_none());
    }

    #[test]
    #[serial]
    fn entries_lists_all_registered() {
        clear();
        register("a", dummy_plugin());
        register("b", dummy_plugin());
        register("c", dummy_plugin());
        let mut labels: Vec<String> = entries().into_iter().map(|(k, _)| k).collect();
        labels.sort();
        assert_eq!(labels, vec!["a", "b", "c"]);
        clear();
    }

    #[test]
    #[serial]
    fn clear_drains_registry() {
        clear();
        register("x", dummy_plugin());
        register("y", dummy_plugin());
        assert_eq!(len(), 2);
        clear();
        assert_eq!(len(), 0);
        assert!(entries().is_empty());
    }

    #[test]
    #[serial]
    fn register_overwrites_same_key() {
        clear();
        let first = dummy_plugin();
        let second = dummy_plugin();
        register("dup", first.clone());
        register("dup", second.clone());
        let got = lookup("dup").expect("registered");
        assert!(
            Arc::ptr_eq(&second, &got),
            "second register must overwrite first"
        );
        assert!(!Arc::ptr_eq(&first, &got));
        clear();
    }

    #[test]
    #[serial]
    fn lookup_unknown_returns_none() {
        clear();
        assert!(lookup("never-registered").is_none());
    }
}
