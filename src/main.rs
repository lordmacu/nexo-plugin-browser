//! Subprocess entrypoint for `nexo-plugin-browser`.
//!
//! Wires:
//!   - [`PluginAdapter`] — child-side JSON-RPC dispatch loop.
//!   - [`browser_tool_defs`] — the `browser_*` tool defs advertised
//!     in the initialize reply.
//!   - [`dispatch_browser_tool`] — per-tool routing for the
//!     resolved [`BrowserPlugin`].
//!   - [`PROFILES`] — `DashMap<agent_id, ProfileEntry>`
//!     keyed on the sanitised `tool.invoke.agent_id`. First
//!     call per agent lazy-boots Chrome with
//!     `${BASE}/profiles/<agent_id>/`; capped + idle-evicted
//!     per [`profile_limits`].
//!
//! Configuration flows from the daemon via env vars:
//!   * `NEXO_PLUGIN_BROWSER_HEADLESS`
//!   * `NEXO_PLUGIN_BROWSER_USER_DATA_DIR` (BASE for per-agent dirs)
//!   * `NEXO_PLUGIN_BROWSER_CDP_URL`
//!   * `NEXO_PLUGIN_BROWSER_MAX_PROFILES`        (multi-profile cap)
//!   * `NEXO_PLUGIN_BROWSER_PROFILE_IDLE_SECS`   (eviction threshold)
//!   * `NEXO_PLUGIN_BROWSER_MULTI_PROFILE`       (opt-out → single profile)

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use nexo_microapp_sdk::plugin::{PluginAdapter, ToolInvocation, ToolInvocationError};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use nexo_plugin_browser::{
    browser_tool_defs,
    dispatch::{dispatch_browser_tool, resolve_plugin_or_legacy},
    env_config::browser_config_from_env,
    profile::{sanitize_id, user_data_dir_for},
    profile_decoration::decorate_profile_dir,
    profile_limits::{read_profile_limits, ProfileLimits},
    BrowserPlugin,
};

const MANIFEST: &str = include_str!("../nexo-plugin.toml");

/// Per-agent profile entry — Arc-shared `BrowserPlugin` plus the
/// timestamp the eviction loop inspects.
struct ProfileEntry {
    plugin: Arc<BrowserPlugin>,
    /// Wall-clock instant of the last successful `tool.invoke`
    /// for this agent. Eviction loop reads, dispatcher writes
    /// (on success only — failed calls preserve the clock).
    last_active_at: Mutex<Instant>,
}

static PROFILES: Lazy<DashMap<String, ProfileEntry>> = Lazy::new(DashMap::new);

/// Resolve the `BrowserPlugin` for the agent making this
/// `tool.invoke` call. First call per `agent_id` lazy-boots a
/// fresh Chrome with `${BASE}/profiles/<agent_id>/`; subsequent
/// calls return the cached `Arc`.
///
/// Behaviour:
///   * Empty `agent_id` (or `None` upstream) → `"default"`.
///   * Multi-profile disabled → key fixed at `"default"`.
///   * Sanitiser rejects → `ArgumentInvalid`.
///   * Cap reached → `Unavailable`.
async fn shared_plugin_for(
    agent_id: &str,
    limits: ProfileLimits,
) -> Result<Arc<BrowserPlugin>, ToolInvocationError> {
    let key = if !limits.multi_profile_enabled || agent_id.is_empty() {
        "default".to_string()
    } else {
        sanitize_id(agent_id)
            .map_err(|e| ToolInvocationError::ArgumentInvalid(e.to_string()))?
    };

    if let Some(entry) = PROFILES.get(&key) {
        return Ok(entry.plugin.clone());
    }

    if PROFILES.len() >= limits.max_profiles {
        return Err(ToolInvocationError::Unavailable(format!(
            "max profiles reached: {}; close idle agents or raise NEXO_PLUGIN_BROWSER_MAX_PROFILES",
            limits.max_profiles
        )));
    }

    // Phase 93.4.e — prefer the configure-delivered cfg when
    // populated; legacy env-var path stays as fallback during the
    // 0.3.x deprecation window.
    let mut cfg = {
        let guard = nexo_plugin_browser::configured_state().read().await;
        if let Some(vec) = guard.as_ref() {
            // Step 4 of browser-multi-instance: cell now holds a
            // `Vec<BrowserConfig>`. The legacy per-agent_id path
            // here predates declared-instance routing (Step 5) and
            // uses the first configured slice as its template. The
            // dispatch resolver (Step 5) only falls into this code
            // path when the instance registry is empty — i.e. when
            // there's at most one effective configuration anyway.
            if let Some(c) = vec.first() {
                c.clone()
            } else {
                drop(guard);
                browser_config_from_env()
            }
        } else {
            drop(guard);
            browser_config_from_env()
        }
    };
    if limits.multi_profile_enabled && key != "default" {
        let base: PathBuf = cfg.user_data_dir.clone().into();
        let derived = user_data_dir_for(&base, &key);
        cfg.user_data_dir = derived.to_string_lossy().into_owned();
        // Best-effort decoration so the Chrome window's profile
        // chip carries the agent's name + a stable color. Errors
        // are operator-visibility, not fatal.
        if let Err(e) = decorate_profile_dir(Path::new(&cfg.user_data_dir), &key).await {
            tracing::warn!(
                agent_id = %key,
                user_data_dir = %cfg.user_data_dir,
                error = %e,
                "decoration failed; continuing without profile chip badge"
            );
        }
    }

    tracing::info!(
        target: "plugin.browser",
        agent_id = %key,
        user_data_dir = %cfg.user_data_dir,
        max_profiles = limits.max_profiles,
        "boot Chrome for agent profile"
    );

    let plugin = Arc::new(BrowserPlugin::new(cfg));
    let entry_ref = PROFILES
        .entry(key)
        .or_insert_with(|| ProfileEntry {
            plugin: plugin.clone(),
            last_active_at: Mutex::new(Instant::now()),
        });
    Ok(entry_ref.plugin.clone())
}

/// Spawn the idle-eviction loop. Polls every 30 s; for each
/// entry whose `last_active_at` exceeds `idle_secs`, calls
/// `shutdown_chrome().await` and removes the DashMap entry.
/// The on-disk profile dir survives — next call lazy-reboots.
fn spawn_idle_eviction_loop(idle_secs: u64) {
    if idle_secs == 0 {
        tracing::debug!(
            target: "plugin.browser",
            "idle eviction disabled (NEXO_PLUGIN_BROWSER_PROFILE_IDLE_SECS=0)"
        );
        return;
    }
    let threshold = Duration::from_secs(idle_secs);
    tokio::spawn(async move {
        let interval = Duration::from_secs(30);
        loop {
            tokio::time::sleep(interval).await;
            let now = Instant::now();
            let mut to_evict: Vec<String> = Vec::new();
            for entry in PROFILES.iter() {
                let last = *entry.value().last_active_at.lock().await;
                if now.duration_since(last) >= threshold {
                    to_evict.push(entry.key().clone());
                }
            }
            for key in to_evict {
                if let Some((_, entry)) = PROFILES.remove(&key) {
                    entry.plugin.shutdown_chrome().await;
                    tracing::info!(
                        target: "plugin.browser",
                        agent_id = %key,
                        idle_secs,
                        "evicted idle Chrome for agent profile"
                    );
                }
            }
        }
    });
}

#[tokio::main]
async fn main() -> nexo_microapp_sdk::Result<()> {
    nexo_microapp_sdk::init_logging_from_env("nexo-plugin-browser");
    let limits = read_profile_limits();
    tracing::info!(
        target: "plugin.browser",
        max_profiles = limits.max_profiles,
        idle_secs = limits.idle_secs,
        multi_profile_enabled = limits.multi_profile_enabled,
        "browser-plugin: profile limits resolved"
    );
    spawn_idle_eviction_loop(limits.idle_secs);

    PluginAdapter::new(MANIFEST)?
        .declare_tools(browser_tool_defs())
        // Phase 93.4.e — receive the operator YAML slice via the
        // host's `plugin.configure` JSON-RPC (Phase 93.2). Single-
        // instance shape per manifest `[plugin.config_schema]
        // shape = "object"`.
        .on_configure(|value: serde_yaml::Value| async move {
            // Accept both the 0.2.x single-map shape and the 0.3.0
            // array shape via the untagged `BrowserPluginShape`
            // enum. Step 6 will rewrite this handler to populate
            // the instance registry; for now we just normalise into
            // the `Vec<BrowserConfig>` cell so the legacy reader
            // above keeps working.
            let shape: nexo_plugin_browser::BrowserPluginShape =
                serde_yaml::from_value(value)
                    .map_err(|e| format!("invalid browser config: {e}"))?;
            *nexo_plugin_browser::configured_state().write().await =
                Some(shape.into_vec());
            Ok(())
        })
        .on_tool(move |inv: ToolInvocation| async move {
            let agent_id = inv.agent_id.as_deref().unwrap_or("");
            // Resolve the legacy per-agent_id plugin eagerly so it's
            // available as the Case-3 fallback in
            // `resolve_plugin_or_legacy`. When the instance registry
            // is populated (declared instances), the legacy plugin
            // is shadowed by the resolver — Chrome stays unbooted
            // for that agent until a tool actually targets it via
            // case 3, since `BrowserPlugin::new` is IO-free.
            let legacy = shared_plugin_for(agent_id, limits).await?;
            let plugin =
                resolve_plugin_or_legacy(&inv.args, agent_id, legacy.clone())?;
            // Re-key the legacy touch lookup using the same logic
            // as shared_plugin_for. Only valid when we actually
            // dispatched against `legacy` — when a declared instance
            // wins, the legacy entry's `last_active_at` shouldn't
            // be bumped (it'd defeat the idle-evict loop's intent).
            let touch_key = if !limits.multi_profile_enabled || agent_id.is_empty() {
                "default".to_string()
            } else {
                sanitize_id(agent_id).unwrap_or_else(|_| "default".into())
            };
            let dispatched_against_legacy = std::sync::Arc::ptr_eq(&plugin, &legacy);
            let result = dispatch_browser_tool(plugin, &inv.tool_name, inv.args).await;
            if result.is_ok() && dispatched_against_legacy {
                if let Some(entry) = PROFILES.get(&touch_key) {
                    *entry.value().last_active_at.lock().await = Instant::now();
                }
            }
            result
        })
        .run_stdio()
        .await
}
