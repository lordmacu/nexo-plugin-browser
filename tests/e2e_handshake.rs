//! End-to-end smoke test: spawn the binary, send `initialize`,
//! assert the reply advertises 12 `browser_*` tools matching the
//! manifest's `extends.tools` allowlist + send a `tool.invoke`
//! for `browser_press_key` (no Chromium needed — fails before
//! Chrome boot at the args-validation guard) and assert the
//! `-33402 ArgumentInvalid` error band fires.
//!
//! Two-tier coverage:
//!
//! ## Structural (default — no `CHROMIUM_BIN` required)
//!
//! Verifies the JSON-RPC dispatch loop:
//!   - `initialize` reply carries `manifest.plugin.id == "browser"`.
//!   - `tools[]` array length == 12.
//!   - Tool names match the canonical sorted list.
//!   - `tool.invoke` for `browser_press_key { key: "" }` (empty
//!     string fails the "single char" guard) returns
//!     `-33402 ArgumentInvalid` BEFORE booting Chrome.
//!
//! ## Live (gated `#[ignore]` — requires `CHROMIUM_BIN`)
//!
//! Verifies real CDP roundtrip:
//!   - `tool.invoke` for `browser_navigate { url: "about:blank" }`
//!     boots Chrome, navigates, returns `{ ok: true }`.
//!   - Skipped on CI without Chromium installed.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");

/// Send a JSON-RPC frame + read one reply line.
fn rpc_round_trip(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    frame: Value,
) -> Value {
    let line = serde_json::to_string(&frame).unwrap();
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let mut buf = String::new();
    stdout.read_line(&mut buf).expect("read reply");
    serde_json::from_str(buf.trim()).expect("reply parses as JSON")
}

#[test]
fn e2e_initialize_advertises_twelve_browser_tools() {
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let reply = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );

    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["manifest"]["plugin"]["id"], "browser");
    let tools = reply["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 12, "expected 12 browser_* tools");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    // Canonical sorted order — matches src/tool_defs.rs.
    assert_eq!(
        names,
        [
            "browser_click",
            "browser_current_url",
            "browser_evaluate",
            "browser_fill",
            "browser_go_back",
            "browser_go_forward",
            "browser_navigate",
            "browser_press_key",
            "browser_screenshot",
            "browser_scroll_to",
            "browser_snapshot",
            "browser_wait_for",
        ]
    );
    // Every tool def carries an object schema.
    for t in tools {
        assert_eq!(t["input_schema"]["type"], "object",
            "tool {} must have object schema", t["name"]);
    }

    // Clean shutdown.
    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
fn e2e_tool_invoke_invalid_args_returns_minus_33402_before_chrome_boot() {
    // Verifies the dispatch path WITHOUT triggering Chrome:
    // `browser_press_key { key: "" }` fails the
    // "single char or known name" guard in dispatch.rs BEFORE
    // calling `plugin.execute(...)`. So no Chromium binary is
    // required — pure SDK-level dispatch validation.
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    // Initialize first to satisfy the manifest's reserved-id-1.
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    // Empty string is neither a known key name nor a single
    // non-control char → guard rejects → -33402.
    let reply = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_press_key",
                "args": { "key": "" },
            }
        }),
    );

    assert_eq!(reply["id"], 5);
    let err = reply["error"].as_object().expect("error envelope");
    assert_eq!(err["code"], -33402, "expected ArgumentInvalid (-33402); got: {err:?}");

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
fn e2e_tool_invoke_unknown_tool_returns_minus_33401() {
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    let reply = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_thirteenth_undefined",
                "args": {},
            }
        }),
    );

    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], -33401, "expected NotFound (-33401)");

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
#[ignore = "requires CHROMIUM_BIN; run with `cargo test -- --ignored`"]
fn e2e_browser_navigate_round_trips() {
    let chromium = std::env::var("CHROMIUM_BIN")
        .expect("CHROMIUM_BIN env unset; install chromium and set the path");
    std::env::set_var("NEXO_PLUGIN_BROWSER_EXECUTABLE", chromium);
    std::env::set_var("NEXO_PLUGIN_BROWSER_HEADLESS", "true");

    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // surface chromium errors to test logs
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    let reply = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_navigate",
                "args": { "url": "about:blank" },
            }
        }),
    );

    assert_eq!(reply["id"], 11);
    let result = reply["result"].as_object().expect("result envelope");
    assert_eq!(result["ok"], true, "browser_navigate failed: {result:?}");

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(5));
}

// ── Helper trait shim — `wait_timeout_or_kill` ────────────────

trait ChildExt {
    fn wait_timeout_or_kill(&mut self, dur: Duration) -> std::io::Result<()>;
}

impl ChildExt for std::process::Child {
    fn wait_timeout_or_kill(&mut self, dur: Duration) -> std::io::Result<()> {
        let deadline = std::time::Instant::now() + dur;
        loop {
            match self.try_wait()? {
                Some(_) => return Ok(()),
                None if std::time::Instant::now() >= deadline => {
                    let _ = self.kill();
                    return self.wait().map(|_| ());
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
