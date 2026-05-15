//! Step 10 of browser-multi-instance — end-to-end multi-instance
//! dispatch routing exercised across the JSON-RPC wire.
//!
//! Spawns the binary, sends initialize, then plugin.configure with
//! a 2-instance array, then tool.invoke frames that prove:
//!   - explicit known instance routes correctly (dispatch error is
//!     the post-routing args guard, not a routing error).
//!   - explicit unknown instance returns ArgumentInvalid with a
//!     message naming the bad label.
//!   - implicit (no `instance` arg) under multi-declared returns
//!     the ambiguity error.
//!
//! Chrome is NOT required — `browser_press_key { key: "" }` rejects
//! at the args guard before Chrome would boot.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");

fn rpc(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>, frame: Value) -> Value {
    let line = serde_json::to_string(&frame).unwrap();
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let mut buf = String::new();
    stdout.read_line(&mut buf).expect("read reply");
    serde_json::from_str(buf.trim()).expect("reply parses as JSON")
}

fn spawn() -> (
    std::process::Child,
    ChildStdin,
    BufReader<ChildStdout>,
) {
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nexo-plugin-browser");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    (child, stdin, stdout)
}

fn configure_two_instances(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    let reply = rpc(
        stdin,
        stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "plugin.configure",
            "params": {
                "value": [
                    { "instance": "alpha", "headless": true },
                    { "instance": "beta", "headless": true }
                ]
            }
        }),
    );
    // configure replies should be successful (no error envelope).
    assert!(
        reply["error"].is_null(),
        "plugin.configure failed: {reply}"
    );
}

fn shutdown(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    let _ = rpc(
        stdin,
        stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
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

#[test]
fn e2e_explicit_known_instance_passes_routing_then_hits_args_guard() {
    let (mut child, mut stdin, mut stdout) = spawn();
    let _ = rpc(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    configure_two_instances(&mut stdin, &mut stdout);

    let reply = rpc(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_press_key",
                "args": { "instance": "alpha", "key": "" }
            }
        }),
    );
    assert_eq!(reply["id"], 5);
    let err = reply["error"].as_object().expect("error envelope");
    assert_eq!(err["code"], -33402);
    // Routing succeeded — the args guard fires with the press_key
    // message, NOT the "instance not declared" message.
    let msg = err["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("press_key") || msg.contains("known key") || msg.contains("single character"),
        "expected press_key args-guard message; got: {msg}"
    );
    assert!(
        !msg.contains("not declared"),
        "explicit known instance must NOT trigger routing error; got: {msg}"
    );

    shutdown(&mut stdin, &mut stdout);
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
fn e2e_explicit_unknown_instance_returns_argument_invalid_with_label() {
    let (mut child, mut stdin, mut stdout) = spawn();
    let _ = rpc(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    configure_two_instances(&mut stdin, &mut stdout);

    let reply = rpc(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_press_key",
                "args": { "instance": "ghost", "key": "Enter" }
            }
        }),
    );
    let err = reply["error"].as_object().expect("error envelope");
    assert_eq!(err["code"], -33402);
    let msg = err["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("ghost"),
        "error must echo unknown label `ghost`; got: {msg}"
    );
    assert!(
        msg.contains("not declared"),
        "error must mention routing rejection; got: {msg}"
    );

    shutdown(&mut stdin, &mut stdout);
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}

#[test]
fn e2e_implicit_under_multi_declared_returns_ambiguous() {
    let (mut child, mut stdin, mut stdout) = spawn();
    let _ = rpc(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    configure_two_instances(&mut stdin, &mut stdout);

    let reply = rpc(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_press_key",
                "args": { "key": "Enter" } // no instance arg
            }
        }),
    );
    let err = reply["error"].as_object().expect("error envelope");
    assert_eq!(err["code"], -33402);
    let msg = err["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("multiple instances"),
        "error must mention ambiguity; got: {msg}"
    );

    shutdown(&mut stdin, &mut stdout);
    let _ = child.wait_timeout_or_kill(Duration::from_secs(2));
}
