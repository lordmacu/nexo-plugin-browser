//! Step 6 of browser-multi-instance — boot-loop for the
//! `plugin.configure` JSON-RPC payload.
//!
//! Lives in the lib (not `main.rs`) so integration tests can drive
//! it directly. `main.rs::on_configure` is now a thin wrapper.

use std::collections::HashSet;
use std::sync::Arc;

use crate::config::{BrowserConfig, BrowserPluginShape};
use crate::plugin::BrowserPlugin;
use crate::profile::sanitize_id;
use crate::{configured_state, instance_registry};

const DEFAULT_USER_DATA_DIR_SENTINEL: &str = "./data/browser/profile";

/// Apply an `plugin.configure` payload.
///
/// - `Single` shape ⇒ legacy back-compat: populate the `configured_state`
///   cell so `shared_plugin_for` (the per-agent_id legacy path) keeps
///   working. Any prior registry entries are unregistered + their
///   Chrome handles shut down (transition from multi-instance back to
///   legacy single).
/// - `Many` shape ⇒ declared-instance path: register an
///   `Arc<BrowserPlugin>` per entry in `instance_registry`. Diff vs
///   prior labels and shut down removed ones.
pub async fn apply_configure(value: serde_yaml::Value) -> Result<(), String> {
    let shape: BrowserPluginShape = serde_yaml::from_value(value)
        .map_err(|e| format!("invalid browser config: {e}"))?;

    let configs: Vec<BrowserConfig> = match shape {
        BrowserPluginShape::Single(c) => {
            shutdown_all_registered().await;
            *configured_state().write().await = Some(vec![c]);
            return Ok(());
        }
        BrowserPluginShape::Many(v) => v,
    };

    let state_root = std::env::var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR")
        .unwrap_or_else(|_| "./data/browser".to_string());

    let mut resolved: Vec<(String, BrowserConfig)> = Vec::with_capacity(configs.len());
    let mut seen: HashSet<String> = HashSet::new();
    for mut cfg in configs.into_iter() {
        let raw_label = cfg.instance.clone().unwrap_or_else(|| "default".into());
        let label = sanitize_id(&raw_label).map_err(|e| {
            format!("invalid browser instance label `{raw_label}`: {e}")
        })?;
        if !seen.insert(label.clone()) {
            return Err(format!("duplicate browser instance label: `{label}`"));
        }
        if cfg.user_data_dir.is_empty()
            || cfg.user_data_dir == DEFAULT_USER_DATA_DIR_SENTINEL
        {
            cfg.user_data_dir = format!("{state_root}/instances/{label}");
        }
        // Keep `instance` reflecting the resolved label so admin RPC
        // replies + config snapshots are self-describing.
        cfg.instance = Some(label.clone());
        resolved.push((label, cfg));
    }

    let prev: HashSet<String> = instance_registry::entries()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let now: HashSet<String> = resolved.iter().map(|(k, _)| k.clone()).collect();
    for stale in prev.difference(&now) {
        if let Some(p) = instance_registry::unregister(stale) {
            p.shutdown_chrome().await;
            tracing::info!(
                target: "plugin.browser",
                instance = %stale,
                "unregistered + shut down stale instance"
            );
        }
    }

    let mut snapshot: Vec<BrowserConfig> = Vec::with_capacity(resolved.len());
    for (label, cfg) in resolved {
        snapshot.push(cfg.clone());
        let plugin = Arc::new(BrowserPlugin::new(cfg));
        instance_registry::register(&label, plugin);
        tracing::info!(
            target: "plugin.browser",
            instance = %label,
            "registered declared instance"
        );
    }
    *configured_state().write().await = Some(snapshot);
    Ok(())
}

async fn shutdown_all_registered() {
    for (label, _) in instance_registry::entries() {
        if let Some(p) = instance_registry::unregister(&label) {
            p.shutdown_chrome().await;
        }
    }
}
