//! Follow-up `browser.auto_discovery.subscriber` — end-to-end
//! coverage for the broker subscriber loop wired in `main.rs`.
//!
//! Spawns the binary in `stdio_bridge` broker mode, publishes a
//! request via the bridge, and asserts the reply lands. Proves the
//! Stage 1+2+4+5 wire is plumbed plugin-side.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");

fn rpc(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
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
fn e2e_subscriber_spawns_without_broker_url_does_not_kill_process() {
    // No NEXO_BROKER_URL + no NEXO_BROKER_KIND ⇒ the subscriber
    // spawn warns + skips; the JSON-RPC tool.invoke path must
    // still answer. This is the most common dev-host scenario.
    let mut child = Command::new(BINARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("NEXO_BROKER_KIND")
        .env_remove("NEXO_BROKER_URL")
        .spawn()
        .expect("spawn nexo-plugin-browser");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let reply = rpc(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    assert_eq!(reply["result"]["manifest"]["plugin"]["id"], "browser");

    // tool.invoke still functional: empty-key press rejects with
    // ArgumentInvalid (-33402) BEFORE Chrome boot.
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
                "args": { "key": "" }
            }
        }),
    );
    assert_eq!(reply["error"]["code"], -33402);

    let _ = rpc(
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
