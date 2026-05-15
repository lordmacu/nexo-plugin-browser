//! Step 7 of browser-multi-instance — auto-discovery broker
//! request handlers.
//!
//! Pure-fn handlers per the Phase 81.33.b.real Stages 1+2+4+5
//! contract. Each takes a `serde_json::Value` request payload and
//! returns the JSON reply.
//!
//! Wiring (the broker subscriber loop that pumps requests through
//! these handlers) is a deferred follow-up
//! `browser.auto_discovery.subscriber` — it requires a broker
//! handle which `nexo-rs-plugin-browser` does not currently construct
//! (it only handles `tool.invoke` via PluginAdapter). The manifest
//! declarations in Step 8 give daemon-side helpers the routing
//! information; the plugin's subscriber loop will be wired in a
//! follow-up commit so this PR stays bounded.

use serde_json::{json, Value};

use crate::admin;

// ── Stage 1 — pairing adapter ──────────────────────────────────

/// Canonicalise an inbound browser "sender" id. Browser is
/// outbound-only — no real notion of "sender". Echoes the input
/// so daemon-side normaliser tooling can stay generic.
///
/// Request: `{ "raw": "<raw>" }`
/// Reply:   `{ "normalized": "<raw>" }`
pub fn pairing_normalize_sender(request: &Value) -> Value {
    let raw = request.get("raw").and_then(|v| v.as_str()).unwrap_or("");
    if raw.is_empty() {
        return json!({ "normalized": null });
    }
    json!({ "normalized": raw })
}

/// Reply delivery is meaningless for an outbound-only channel.
/// Returns a structured "not supported" reply so callers see why.
pub async fn pairing_send_reply(_request: &Value) -> Value {
    json!({
        "ok": false,
        "error": "browser plugin is outbound-only; pairing.send_reply not supported",
    })
}

/// Browser pairing uses `kind = "form"` (operator logs in manually),
/// not `kind = "qr"`. Daemon-side QR helpers should never route
/// here, but if they do the reply is explicit.
pub async fn pairing_send_qr_image(_request: &Value) -> Value {
    json!({
        "ok": false,
        "error": "browser pairing uses kind=form, not qr; send_qr_image not supported",
    })
}

// ── Stage 2 — HTTP routes ──────────────────────────────────────

/// Browser plugin currently exposes no HTTP routes — declared for
/// daemon contract completeness only. Replies 501.
pub async fn http_request(request: &Value) -> Value {
    let path = request.get("path").and_then(|v| v.as_str()).unwrap_or("");
    json!({
        "status": 501,
        "headers": { "content-type": "text/plain" },
        "body_base64": base64_text(&format!(
            "browser plugin declares no HTTP routes (requested `{path}`)\n"
        )),
    })
}

fn base64_text(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

// ── Stage 4 — admin RPC ────────────────────────────────────────

/// Parse `{ "method": "nexo/admin/browser/<verb>", "params": {...} }`
/// and dispatch to the matching `admin::*` handler.
pub async fn admin_handle(request: &Value) -> Value {
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let prefix = "nexo/admin/browser/";
    let verb = if let Some(rest) = method.strip_prefix(prefix) {
        rest
    } else {
        return json!({ "ok": false, "error": format!("method `{method}` does not start with `{prefix}`") });
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    admin::dispatch(verb, &params).await
}

// ── Stage 5 — Prometheus metrics scrape ────────────────────────

/// Step 9 — encode the per-instance Prometheus registry.
pub async fn metrics_scrape(_request: &Value) -> Value {
    let mut body = String::from(
        "# HELP browser_plugin_ready Plugin process up.\n\
         # TYPE browser_plugin_ready gauge\n\
         browser_plugin_ready 1\n",
    );
    body.push_str(&crate::metrics::scrape());
    json!({ "text": body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BrowserConfig;
    use crate::instance_registry;
    use crate::plugin::BrowserPlugin;
    use base64::Engine;
    use serial_test::serial;
    use std::sync::Arc;

    fn register(label: &str, dir: &str) {
        instance_registry::register(
            label,
            Arc::new(BrowserPlugin::new(BrowserConfig {
                instance: Some(label.into()),
                headless: true,
                executable: String::new(),
                cdp_url: String::new(),
                user_data_dir: dir.into(),
                window_width: 1280,
                window_height: 800,
                connect_timeout_ms: 8_000,
                command_timeout_ms: 30_000,
                args: Vec::new(),
                allow_agents: Vec::new(),
            })),
        );
    }

    #[test]
    #[serial]
    fn pairing_normalize_sender_echoes_input() {
        let r = pairing_normalize_sender(&json!({ "raw": "abc-123" }));
        assert_eq!(r["normalized"].as_str(), Some("abc-123"));
    }

    #[test]
    #[serial]
    fn pairing_normalize_sender_empty_returns_null() {
        let r = pairing_normalize_sender(&json!({ "raw": "" }));
        assert!(r["normalized"].is_null());
    }

    #[tokio::test]
    #[serial]
    async fn pairing_send_reply_returns_unsupported() {
        let r = pairing_send_reply(&json!({})).await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert!(r["error"].as_str().unwrap().contains("outbound-only"));
    }

    #[tokio::test]
    #[serial]
    async fn pairing_send_qr_image_returns_unsupported() {
        let r = pairing_send_qr_image(&json!({})).await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert!(r["error"].as_str().unwrap().contains("form"));
    }

    #[tokio::test]
    #[serial]
    async fn http_request_returns_501() {
        let r = http_request(&json!({ "method": "GET", "path": "/browser/anything" })).await;
        assert_eq!(r["status"].as_u64(), Some(501));
        let body = base64::engine::general_purpose::STANDARD
            .decode(r["body_base64"].as_str().unwrap())
            .unwrap();
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("declares no HTTP routes"));
    }

    #[tokio::test]
    #[serial]
    async fn admin_dispatch_lists_registered_instances() {
        instance_registry::clear();
        let tmp = tempfile::tempdir().unwrap();
        register("alpha", tmp.path().to_str().unwrap());
        register("beta", tmp.path().join("beta").to_str().unwrap());

        let r = admin_handle(&json!({
            "method": "nexo/admin/browser/list_instances",
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        let arr = r["result"]["instances"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        instance_registry::clear();
    }

    #[tokio::test]
    #[serial]
    async fn admin_dispatch_unknown_method_errors() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/whatsapp/list", // wrong prefix
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert!(r["error"].as_str().unwrap().contains("does not start"));
    }

    #[tokio::test]
    #[serial]
    async fn admin_dispatch_unknown_verb_errors() {
        let r = admin_handle(&json!({
            "method": "nexo/admin/browser/nonexistent",
            "params": {},
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert!(r["error"].as_str().unwrap().contains("unknown admin verb"));
    }

    #[tokio::test]
    #[serial]
    async fn admin_mark_paired_writes_sentinel_then_list_reports_paired() {
        instance_registry::clear();
        let tmp = tempfile::tempdir().unwrap();
        register("alpha", tmp.path().to_str().unwrap());

        let r = admin_handle(&json!({
            "method": "nexo/admin/browser/mark_paired",
            "params": { "instance": "alpha" },
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true), "got: {r:?}");
        assert!(tmp.path().join(".nexo-paired").exists());

        let listed = admin_handle(&json!({
            "method": "nexo/admin/browser/list_instances",
            "params": {},
        }))
        .await;
        let arr = listed["result"]["instances"].as_array().unwrap();
        assert_eq!(arr[0]["paired"].as_bool(), Some(true));
        instance_registry::clear();
    }

    #[tokio::test]
    #[serial]
    async fn admin_shutdown_unknown_instance_errors() {
        instance_registry::clear();
        let r = admin_handle(&json!({
            "method": "nexo/admin/browser/shutdown",
            "params": { "instance": "ghost" },
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(false));
        assert!(r["error"].as_str().unwrap().contains("not declared"));
    }

    #[tokio::test]
    #[serial]
    async fn admin_shutdown_idempotent_on_fresh_plugin() {
        instance_registry::clear();
        let tmp = tempfile::tempdir().unwrap();
        register("alpha", tmp.path().to_str().unwrap());

        for _ in 0..3 {
            let r = admin_handle(&json!({
                "method": "nexo/admin/browser/shutdown",
                "params": { "instance": "alpha" },
            }))
            .await;
            assert_eq!(r["ok"].as_bool(), Some(true), "got: {r:?}");
        }
        instance_registry::clear();
    }

    #[tokio::test]
    #[serial]
    async fn admin_launch_visible_returns_stub_ack() {
        instance_registry::clear();
        let tmp = tempfile::tempdir().unwrap();
        register("alpha", tmp.path().to_str().unwrap());

        let r = admin_handle(&json!({
            "method": "nexo/admin/browser/launch_visible",
            "params": { "instance": "alpha" },
        }))
        .await;
        assert_eq!(r["ok"].as_bool(), Some(true));
        assert_eq!(r["result"]["launched"].as_bool(), Some(false));
        instance_registry::clear();
    }

    #[tokio::test]
    #[serial]
    async fn metrics_scrape_returns_ready_gauge() {
        let r = metrics_scrape(&json!({})).await;
        let text = r["text"].as_str().expect("text");
        assert!(text.contains("browser_plugin_ready 1"));
        assert!(text.contains("# TYPE browser_plugin_ready gauge"));
    }
}
