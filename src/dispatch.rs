//! Subprocess-side tool dispatcher.
//!
//! Maps each `browser_*` tool name to a [`BrowserPlugin::execute`]
//! flow without requiring an `AgentContext` (which the subprocess
//! cannot construct). Tool argument schemas live in
//! `tool_defs.rs` — keep the two in sync when adding or changing
//! a tool.

use std::sync::Arc;

use anyhow::anyhow;
use nexo_microapp_sdk::plugin::ToolInvocationError;
use serde_json::{json, Value};

use crate::command::{BrowserCmd, BrowserResult};
use crate::plugin::BrowserPlugin;

/// Convert a [`BrowserResult`] into the JSON value that lands in
/// a `tool.invoke` reply's `result` field.
fn result_to_json(res: BrowserResult) -> Value {
    if !res.ok {
        return json!({
            "ok": false,
            "error": res.error.unwrap_or_else(|| "unknown error".into()),
        });
    }
    let mut out = json!({ "ok": true });
    let obj = out.as_object_mut().unwrap();
    if let Some(r) = res.result {
        obj.insert("result".into(), r);
    }
    if let Some(d) = res.data {
        obj.insert("png_base64".into(), Value::String(d));
    }
    if let Some(s) = res.snapshot {
        obj.insert("snapshot".into(), Value::String(s));
    }
    out
}

/// Allowed named keys for `browser_press_key`.
const ALLOWED_KEY_NAMES: &[&str] = &[
    "Enter", "Tab", "Escape",
    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
    "Backspace", "Delete", "Home", "End", "PageUp", "PageDown",
    "Space",
];

/// Match `tool_name` against the `browser_*` tools and dispatch
/// to the matching `BrowserPlugin::execute` flow. Returns a JSON
/// `Value` ready to land in the `tool.invoke` reply's `result`
/// field, or [`ToolInvocationError`] for the host's
/// `RemoteToolHandler` decoder.
pub async fn dispatch_browser_tool(
    plugin: Arc<BrowserPlugin>,
    tool_name: &str,
    args: Value,
) -> Result<Value, ToolInvocationError> {
    match tool_name {
        "browser_navigate" => {
            let url = arg_string(&args, "url", "browser_navigate")?;
            Ok(result_to_json(
                plugin.execute(BrowserCmd::Navigate { url }).await,
            ))
        }
        "browser_click" => {
            let target = arg_string(&args, "target", "browser_click")?;
            Ok(result_to_json(
                plugin.execute(BrowserCmd::Click { target }).await,
            ))
        }
        "browser_fill" => {
            let target = arg_string(&args, "target", "browser_fill")?;
            let value = arg_string(&args, "value", "browser_fill")?;
            Ok(result_to_json(
                plugin.execute(BrowserCmd::Fill { target, value }).await,
            ))
        }
        "browser_screenshot" => {
            Ok(result_to_json(plugin.execute(BrowserCmd::Screenshot).await))
        }
        "browser_evaluate" => {
            let script = arg_string(&args, "script", "browser_evaluate")?;
            Ok(result_to_json(
                plugin.execute(BrowserCmd::Evaluate { script }).await,
            ))
        }
        "browser_snapshot" => {
            Ok(result_to_json(plugin.execute(BrowserCmd::Snapshot).await))
        }
        "browser_scroll_to" => {
            let target = arg_string(&args, "target", "browser_scroll_to")?;
            Ok(result_to_json(
                plugin.execute(BrowserCmd::ScrollTo { target }).await,
            ))
        }
        "browser_current_url" => {
            Ok(result_to_json(
                plugin
                    .execute(BrowserCmd::Evaluate {
                        script: "location.href".into(),
                    })
                    .await,
            ))
        }
        "browser_wait_for" => {
            let selector = arg_string(&args, "selector", "browser_wait_for")?;
            let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(5_000).min(30_000);
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
            loop {
                let script = format!(
                    "!!document.querySelector({})",
                    serde_json::to_string(&selector)
                        .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?
                );
                let res = plugin.execute(BrowserCmd::Evaluate { script }).await;
                if res.ok {
                    if let Some(Value::Bool(true)) = res.result.as_ref() {
                        return Ok(json!({"ok": true, "found": true}));
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Ok(json!({"ok": true, "found": false}));
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
        "browser_go_back" => {
            Ok(result_to_json(
                plugin
                    .execute(BrowserCmd::Evaluate {
                        script: "history.back()".into(),
                    })
                    .await,
            ))
        }
        "browser_go_forward" => {
            Ok(result_to_json(
                plugin
                    .execute(BrowserCmd::Evaluate {
                        script: "history.forward()".into(),
                    })
                    .await,
            ))
        }
        "browser_press_key" => {
            let key = args["key"]
                .as_str()
                .ok_or_else(|| {
                    ToolInvocationError::ArgumentInvalid(
                        "browser_press_key requires `key`".into(),
                    )
                })?
                .to_string();
            let named = ALLOWED_KEY_NAMES.contains(&key.as_str());
            let is_single_char = key.chars().count() == 1
                && key.chars().next().map(|c| !c.is_control()).unwrap_or(false);
            if !named && !is_single_char {
                return Err(ToolInvocationError::ArgumentInvalid(format!(
                    "browser_press_key rejected: expected a known key name or a single character, got `{key}`"
                )));
            }
            let script = format!(
                r#"(function() {{
                    const k = {};
                    const map = {{
                        Enter:      {{ key: 'Enter',      code: 'Enter',      keyCode: 13 }},
                        Tab:        {{ key: 'Tab',        code: 'Tab',        keyCode: 9  }},
                        Escape:     {{ key: 'Escape',     code: 'Escape',     keyCode: 27 }},
                        ArrowUp:    {{ key: 'ArrowUp',    code: 'ArrowUp',    keyCode: 38 }},
                        ArrowDown:  {{ key: 'ArrowDown',  code: 'ArrowDown',  keyCode: 40 }},
                        ArrowLeft:  {{ key: 'ArrowLeft',  code: 'ArrowLeft',  keyCode: 37 }},
                        ArrowRight: {{ key: 'ArrowRight', code: 'ArrowRight', keyCode: 39 }}
                    }};
                    const spec = map[k] || {{ key: k, code: k, keyCode: k.charCodeAt(0) }};
                    const target = document.activeElement || document.body;
                    target.dispatchEvent(new KeyboardEvent('keydown', {{ ...spec, bubbles: true }}));
                    target.dispatchEvent(new KeyboardEvent('keyup',   {{ ...spec, bubbles: true }}));
                    return true;
                }})()"#,
                serde_json::to_string(&key)
                    .map_err(|e| ToolInvocationError::ExecutionFailed(e.to_string()))?
            );
            Ok(result_to_json(
                plugin.execute(BrowserCmd::Evaluate { script }).await,
            ))
        }
        other => Err(ToolInvocationError::NotFound(other.to_string())),
    }
}

fn arg_string(
    args: &Value,
    field: &str,
    tool_name: &str,
) -> Result<String, ToolInvocationError> {
    args[field]
        .as_str()
        .ok_or_else(|| {
            ToolInvocationError::ArgumentInvalid(format!(
                "{tool_name} requires `{field}` (string)"
            ))
        })
        .map(str::to_string)
}

// Bridge to anyhow for tests if ever needed.
#[allow(dead_code)]
fn anyhow_to_invocation(e: anyhow::Error) -> ToolInvocationError {
    ToolInvocationError::ExecutionFailed(format!("{e:#}"))
}

// Suppress unused import warning when anyhow is not invoked.
#[allow(dead_code)]
fn _suppress() -> anyhow::Error {
    anyhow!("unused")
}
