//! Step 7 of browser-multi-instance — admin RPC handlers.
//!
//! Pure-fn handlers (one per verb) that operate on
//! [`instance_registry`] + filesystem sentinel files. They take a
//! `serde_json::Value` request payload and return the JSON reply.
//!
//! Wiring is split:
//! - Handlers (this file) — testable in-process, no broker dep.
//! - Broker subscriber loop — see `auto_discovery.rs` + main.rs
//!   wire-up (deferred follow-up `browser.auto_discovery.subscriber`).
//!
//! Daemon-side admin RPC `nexo/admin/browser/<verb>` is forwarded
//! via the broker pattern declared in `nexo-plugin.toml`
//! `[plugin.admin]` (Step 8).

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::instance_registry;

const PAIRED_SENTINEL: &str = ".nexo-paired";

fn ok(result: Value) -> Value {
    json!({ "ok": true, "result": result })
}

fn err(msg: impl Into<String>) -> Value {
    json!({ "ok": false, "error": msg.into() })
}

fn arg_instance(req: &Value) -> Result<String, Value> {
    req.get("instance")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| err("`instance` arg required"))
}

/// `nexo/admin/browser/list_instances` — enumerate every declared
/// instance with its current paired state.
pub fn list_instances() -> Value {
    let rows: Vec<Value> = instance_registry::entries()
        .into_iter()
        .map(|(label, plugin)| {
            let dir = plugin.user_data_dir().to_string();
            let paired = PathBuf::from(&dir).join(PAIRED_SENTINEL).exists();
            json!({
                "instance": label,
                "user_data_dir": dir,
                "paired": paired,
            })
        })
        .collect();
    ok(json!({ "instances": rows }))
}

/// `nexo/admin/browser/shutdown` — shut down the Chrome handle for
/// a specific instance. Idempotent. The registry entry stays so
/// the next `tool.invoke` lazy-reboots Chrome.
pub async fn shutdown(req: &Value) -> Value {
    let instance = match arg_instance(req) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(plugin) = instance_registry::lookup(&instance) else {
        return err(format!("instance `{instance}` not declared"));
    };
    plugin.shutdown_chrome().await;
    ok(json!({ "instance": instance, "shutdown": true }))
}

/// `nexo/admin/browser/restart` — shutdown + register intent to
/// re-boot. The actual Chrome boot is lazy (next `tool.invoke`),
/// so this verb is effectively `shutdown` from the runtime's
/// perspective.
pub async fn restart(req: &Value) -> Value {
    let instance = match arg_instance(req) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(plugin) = instance_registry::lookup(&instance) else {
        return err(format!("instance `{instance}` not declared"));
    };
    plugin.shutdown_chrome().await;
    ok(json!({ "instance": instance, "restart": true }))
}

/// `nexo/admin/browser/mark_paired` — touches the `.nexo-paired`
/// sentinel inside the instance's `user_data_dir` so the dashboard
/// `auth_check` (Stage 6 wizard) flips green. Operator-confirmed
/// signal; replaces fragile Chrome-internal SQLite probing.
pub fn mark_paired(req: &Value) -> Value {
    let instance = match arg_instance(req) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(plugin) = instance_registry::lookup(&instance) else {
        return err(format!("instance `{instance}` not declared"));
    };
    let dir = PathBuf::from(plugin.user_data_dir());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err(format!("create user_data_dir failed: {e}"));
    }
    let path = dir.join(PAIRED_SENTINEL);
    match std::fs::write(&path, b"paired\n") {
        Ok(()) => ok(json!({
            "instance": instance,
            "sentinel": path.to_string_lossy(),
        })),
        Err(e) => err(format!("write sentinel failed: {e}")),
    }
}

/// `nexo/admin/browser/launch_visible` — request a non-headless
/// Chrome boot for the instance so the operator can complete a
/// manual pairing flow. Step 7 ships the wiring contract only —
/// actual headless-override on lazy-boot lands as a deferred
/// follow-up (`browser.launch_visible.runtime`). Until then this
/// verb is a no-op acknowledgement.
pub fn launch_visible(req: &Value) -> Value {
    let instance = match arg_instance(req) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if instance_registry::lookup(&instance).is_none() {
        return err(format!("instance `{instance}` not declared"));
    }
    ok(json!({
        "instance": instance,
        "launched": false,
        "note": "stub — runtime headless-override is a deferred follow-up",
    }))
}

/// Generic dispatch entry point — matches the broker topic suffix
/// to a handler. Unknown verb ⇒ structured error.
pub async fn dispatch(verb: &str, params: &Value) -> Value {
    match verb {
        "list_instances" => list_instances(),
        "shutdown" => shutdown(params).await,
        "restart" => restart(params).await,
        "mark_paired" => mark_paired(params),
        "launch_visible" => launch_visible(params),
        other => err(format!("unknown admin verb: `{other}`")),
    }
}
