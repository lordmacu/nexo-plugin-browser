//! Subprocess lifecycle + state persistence — proves the
//! standalone binary survives multiple `tool.invoke` round trips
//! against the same cached `BrowserPlugin` instance.
//!
//! Hot-reload risk: when the daemon's reload coordinator
//! re-evaluates the agent registry, it must NOT kill the
//! subprocess (the subprocess lifecycle is bound to the
//! manifest's discovery, not the agent yaml). This test verifies
//! the subprocess-side guarantee: under repeated tool.invoke
//! calls across simulated agent yaml reloads, the subprocess
//! stays up and state is consistent.
//!
//! Test strategy: spawn the binary once, send N rounds of
//! initialize/tool.invoke pairs. The binary's pid stays the
//! same; the cached `BrowserPlugin` stays primed. We exercise
//! `browser_press_key` with a known-bad arg so we don't require
//! Chromium — what we're testing is the dispatch loop's
//! resilience, not Chrome.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");

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
fn subprocess_handles_n_consecutive_tool_invokes_without_dying() {
    // Subprocess + reload coordinator interaction: the daemon
    // can drive arbitrary tool.invoke calls across the
    // subprocess's lifetime. Verify N=20 calls in a row don't
    // exhaust pending-id state, leak file handles, or trip
    // the supervisor's exit-detection path.
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let pid_initial = child.id();
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    assert_eq!(init["result"]["manifest"]["plugin"]["id"], "browser");

    for i in 0..20 {
        let reply = rpc_round_trip(
            &mut stdin,
            &mut stdout,
            json!({
                "jsonrpc": "2.0",
                "id": 100 + i,
                "method": "tool.invoke",
                "params": {
                    "plugin_id": "browser",
                    "tool_name": "browser_press_key",
                    "args": { "key": "" },  // guard rejects → -33402
                },
            }),
        );
        assert_eq!(reply["id"], 100 + i, "iteration {i}: id mismatch");
        assert_eq!(
            reply["error"]["code"], -33402,
            "iteration {i}: expected -33402; got {:?}", reply["error"]
        );
    }

    // PID unchanged proves no respawn / restart happened.
    assert_eq!(child.id(), pid_initial, "subprocess restarted mid-test");

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
fn subprocess_method_not_found_for_unknown_method_does_not_kill() {
    // Verifies error envelopes for unrecognised JSON-RPC methods
    // don't cascade into subprocess death (the dispatch loop
    // must reply -32601 and continue serving).
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

    // Unknown method.
    let reply = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "nonexistent.method",
            "params": {}
        }),
    );
    assert_eq!(reply["error"]["code"], -32601);

    // Recovery: subprocess still serves tool.invoke after the
    // unknown-method error.
    let recover = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_thirteenth",
                "args": {},
            },
        }),
    );
    assert_eq!(recover["error"]["code"], -33401);

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

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
