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
use nexo_broker::{AnyBroker, BrokerHandle, Event, Message, StdioBridgeBroker};
use nexo_microapp_sdk::plugin::{PluginAdapter, ToolInvocation, ToolInvocationError};
use once_cell::sync::Lazy;
use tokio::sync::{Mutex, OnceCell};

use nexo_plugin_browser::{
    browser_tool_defs,
    dispatch::{dispatch_browser_tool, resolve_plugin_or_legacy_gated},
    env_config::browser_config_from_env,
    profile::{sanitize_id, user_data_dir_for},
    profile_decoration::decorate_profile_dir,
    profile_limits::{read_profile_limits, ProfileLimits},
    BrowserPlugin,
};

const MANIFEST: &str = include_str!("../nexo-plugin.toml");

/// Follow-up `browser.auto_discovery.subscriber` — populated in
/// `main()` when the daemon stamps `NEXO_BROKER_KIND=stdio_bridge`.
/// Mirrors the BRIDGE cell in telegram + whatsapp plugins; same
/// role, same wiring.
static BRIDGE: Lazy<OnceCell<Arc<StdioBridgeBroker>>> = Lazy::new(OnceCell::new);

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

/// Construct the broker handle the auto-discovery subscriber loop
/// reads from. `stdio_bridge` clones from [`BRIDGE`]; anything else
/// (default + explicit `nats`) connects via `NEXO_BROKER_URL`.
async fn auto_discovery_broker() -> anyhow::Result<AnyBroker> {
    let kind = std::env::var("NEXO_BROKER_KIND").unwrap_or_else(|_| "nats".to_string());
    if kind == "stdio_bridge" {
        let bridge = BRIDGE
            .get()
            .ok_or_else(|| anyhow::anyhow!("BRIDGE not initialized"))?;
        return Ok(AnyBroker::stdio_bridge((**bridge).clone()));
    }
    let url = std::env::var("NEXO_BROKER_URL")
        .map_err(|_| anyhow::anyhow!("NEXO_BROKER_URL not set"))?;
    let inner = nexo_config::types::broker::BrokerInner {
        kind: if url.starts_with("nats://") {
            nexo_config::types::broker::BrokerKind::Nats
        } else {
            nexo_config::types::broker::BrokerKind::Local
        },
        url,
        auth: nexo_config::types::broker::BrokerAuthConfig::default(),
        persistence: nexo_config::types::broker::BrokerPersistenceConfig::default(),
        limits: nexo_config::types::broker::BrokerLimitsConfig::default(),
        fallback: nexo_config::types::broker::BrokerFallbackConfig::default(),
    };
    AnyBroker::from_config(&inner)
        .await
        .map_err(|e| anyhow::anyhow!("broker connect failed: {e}"))
}

/// Auto-discovery broker subscriber loop. Spawns one tokio task per
/// request-reply topic family declared in `nexo-plugin.toml`. Each
/// task subscribes, parses [`Message`] from the inbound payload,
/// invokes the matching async handler in
/// [`nexo_plugin_browser::auto_discovery`], and publishes the reply
/// back to `msg.reply_to`. Failure isolation: each task owns its
/// subscription loop; a panic in one does NOT take down the plugin
/// process or sibling tasks.
fn spawn_auto_discovery_subscribers(broker: AnyBroker) {
    use nexo_plugin_browser::auto_discovery as ad;

    spawn_one(
        broker.clone(),
        "plugin.browser.pairing.normalize_sender",
        |_b, p| async move { ad::pairing_normalize_sender(&p) },
    );
    spawn_one(
        broker.clone(),
        "plugin.browser.pairing.send_reply",
        |_b, p| async move { ad::pairing_send_reply(&p).await },
    );
    spawn_one(
        broker.clone(),
        "plugin.browser.pairing.send_qr_image",
        |_b, p| async move { ad::pairing_send_qr_image(&p).await },
    );
    spawn_one(
        broker.clone(),
        "plugin.browser.http.request",
        |_b, p| async move { ad::http_request(&p).await },
    );
    spawn_one(
        broker.clone(),
        "plugin.browser.metrics.scrape",
        |_b, p| async move { ad::metrics_scrape(&p).await },
    );
    spawn_one(broker, "plugin.browser.admin.>", |_b, p| async move {
        ad::admin_handle(&p).await
    });
}

fn spawn_one<F, Fut>(broker: AnyBroker, topic: &'static str, handler: F)
where
    F: Fn(AnyBroker, serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = serde_json::Value> + Send + 'static,
{
    tokio::spawn(async move {
        let mut sub = match broker.subscribe(topic).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target = "browser.auto_discovery",
                    topic,
                    error = %e,
                    "subscribe failed; topic will not receive requests"
                );
                return;
            }
        };
        tracing::info!(target = "browser.auto_discovery", topic, "subscriber up");
        while let Some(event) = sub.next().await {
            let Ok(msg) = serde_json::from_value::<Message>(event.payload) else {
                continue;
            };
            let Some(reply_to) = msg.reply_to.clone() else {
                continue;
            };
            let reply_payload = handler(broker.clone(), msg.payload.clone()).await;
            let reply_msg = Message::new(reply_to.clone(), reply_payload);
            let reply_event = Event::new(
                reply_to.clone(),
                "browser",
                match serde_json::to_value(&reply_msg) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
            );
            if let Err(e) = broker.publish(&reply_to, reply_event).await {
                tracing::warn!(
                    target = "browser.auto_discovery",
                    topic,
                    reply_to = %reply_to,
                    error = %e,
                    "failed to publish reply"
                );
            }
        }
        tracing::debug!(
            target = "browser.auto_discovery",
            topic,
            "subscriber stream ended"
        );
    });
}

#[tokio::main]
async fn main() -> nexo_microapp_sdk::Result<()> {
    // Stage 8 cargo-install ergonomics. When the daemon's binary-
    // mode discovery walker probes us with
    // `nexo-plugin-browser --print-manifest` we emit the bundled
    // TOML to stdout and exit 0 BEFORE init / broker wiring — the
    // walker needs only the manifest bytes.
    nexo_microapp_sdk::plugin::print_manifest_if_requested(MANIFEST);

    nexo_microapp_sdk::init_logging_from_env("nexo-plugin-browser");
    // rustls 0.23 requires an explicit process-wide CryptoProvider
    // before `ClientConfig::builder()` can return successfully.
    // Mirrors the proyecto daemon + telegram/whatsapp plugins.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let limits = read_profile_limits();
    tracing::info!(
        target: "plugin.browser",
        max_profiles = limits.max_profiles,
        idle_secs = limits.idle_secs,
        multi_profile_enabled = limits.multi_profile_enabled,
        "browser-plugin: profile limits resolved"
    );
    spawn_idle_eviction_loop(limits.idle_secs);

    let adapter = PluginAdapter::new(MANIFEST)?
        .declare_tools(browser_tool_defs())
        // Phase 93.4.e — receive the operator YAML slice via the
        // host's `plugin.configure` JSON-RPC (Phase 93.2). Single-
        // instance shape per manifest `[plugin.config_schema]
        // shape = "object"`.
        .on_configure(|value: serde_yaml::Value| async move {
            nexo_plugin_browser::boot::apply_configure(value).await
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
            let plugin = resolve_plugin_or_legacy_gated(
                &inv.args,
                agent_id,
                legacy.clone(),
                limits.legacy_per_agent_enabled,
            )?;
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
            let instance_label = if dispatched_against_legacy {
                "legacy".to_string()
            } else {
                plugin
                    .config_snapshot()
                    .instance
                    .clone()
                    .unwrap_or_else(|| "default".into())
            };
            let tool_name_for_metrics = inv.tool_name.clone();
            let start = Instant::now();
            let result = dispatch_browser_tool(plugin, &inv.tool_name, inv.args).await;
            nexo_plugin_browser::metrics::record_invocation(
                &instance_label,
                &tool_name_for_metrics,
                result.is_ok(),
                start.elapsed().as_secs_f64(),
            );
            if result.is_ok() && dispatched_against_legacy {
                if let Some(entry) = PROFILES.get(&touch_key) {
                    *entry.value().last_active_at.lock().await = Instant::now();
                }
            }
            result
        });

    // Wire the bridge first if the daemon stamped
    // `NEXO_BROKER_KIND=stdio_bridge` so the BRIDGE cell is
    // populated before `auto_discovery_broker()` reads it.
    let adapter = if std::env::var("NEXO_BROKER_KIND").as_deref() == Ok("stdio_bridge") {
        let (adapter, bridge) = adapter.with_stdio_bridge_broker();
        BRIDGE
            .set(bridge.clone())
            .map_err(|_| {
                nexo_microapp_sdk::Error::Internal(
                    "BRIDGE already initialized (this should not happen)".into(),
                )
            })?;
        tracing::info!(
            target = "nexo_plugin_browser",
            "stdio_bridge broker wired (daemon broker = Local)"
        );
        adapter
    } else {
        adapter
    };

    // Follow-up `browser.auto_discovery.subscriber` — spawn the
    // broker subscriber loop unconditionally so both `stdio_bridge`
    // and `nats` modes serve daemon-published Stage 1/2/4/5
    // requests. tool.invoke path stays unaffected if the broker is
    // unreachable.
    match auto_discovery_broker().await {
        Ok(broker) => spawn_auto_discovery_subscribers(broker),
        Err(e) => tracing::warn!(
            target = "nexo_plugin_browser",
            error = %e,
            "auto-discovery broker unavailable; subscribers not spawned (tool.invoke path unaffected)"
        ),
    }

    adapter.run_stdio().await
}
