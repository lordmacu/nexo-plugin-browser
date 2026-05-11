//! End-to-end tests for the per-agent profile DashMap +
//! cap + opt-out + sanitiser rejection paths.
//!
//! Strategy: spawn the binary with carefully chosen
//! `NEXO_PLUGIN_BROWSER_*` env vars, send `tool.invoke` frames
//! against it via stdio, and assert on the JSON-RPC reply codes
//! + the `plugin.browser` log lines on stderr.
//!
//! All tests use `browser_press_key { key: "" }` as the dispatch
//! vector — the press-key guard rejects empty keys with
//! `-33402 ArgumentInvalid` BEFORE Chrome would actually boot,
//! so the tests never need a Chromium binary on the host.
//!
//! Each test spawns a fresh subprocess so DashMap state doesn't
//! leak across tests.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");

/// Spawn the binary with extra env vars on top of the
/// inherited environment. Returns the child + handles to its
/// stdin / stdout / stderr-reader.
fn spawn_with_env(
    extra_env: &[(&str, &str)],
) -> (
    std::process::Child,
    ChildStdin,
    BufReader<ChildStdout>,
    BufReader<std::process::ChildStderr>,
) {
    let mut cmd = Command::new(BINARY);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // tracing-subscriber's fmt layer honours NO_COLOR=1 by
        // suppressing ANSI escapes — without this the boot-log
        // parser sees `agent_id\x1b[0m=\x1b[0m...` and can't split.
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "plugin.browser=info,nexo_plugin_browser=info");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn nexo-plugin-browser");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let stderr = BufReader::new(child.stderr.take().expect("stderr"));
    (child, stdin, stdout, stderr)
}

fn rpc_round_trip(
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

fn invoke_press_key(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    agent_id: Option<&str>,
) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("plugin_id".into(), Value::String("browser".into()));
    params.insert("tool_name".into(), Value::String("browser_press_key".into()));
    params.insert("args".into(), json!({ "key": "" }));
    if let Some(a) = agent_id {
        params.insert("agent_id".into(), Value::String(a.into()));
    }
    rpc_round_trip(
        stdin,
        stdout,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tool.invoke",
            "params": Value::Object(params),
        }),
    )
}

/// Drain stderr lines (non-blocking via a deadline) and collect
/// every `plugin.browser`-targeted INFO line emitted so far.
/// `tracing-subscriber` formats them as text; we filter on the
/// substring `boot Chrome for agent profile`.
///
/// Unix-only: it flips the stderr fd to `O_NONBLOCK` via `libc` so
/// `read_line` returns instead of blocking when nothing is buffered.
/// The Windows pipe API has no drop-in equivalent, so the tests that
/// assert on the boot log are `#[cfg(unix)]` too.
#[cfg(unix)]
fn collect_boot_log_agent_ids(
    stderr: &mut BufReader<std::process::ChildStderr>,
    deadline: Duration,
) -> Vec<String> {
    use std::os::fd::AsRawFd;
    let raw = stderr.get_ref().as_raw_fd();
    // Set non-blocking on the fd so read_line returns
    // immediately when no data is buffered. Restore at the end.
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut acc = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        let mut line = String::new();
        match stderr.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(agent_id) = parse_boot_agent_id(&line) {
                    acc.push(agent_id);
                }
            }
            Err(_) => {
                // EWOULDBLOCK — sleep briefly and retry.
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        libc::fcntl(raw, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
    acc
}

#[cfg(unix)]
fn parse_boot_agent_id(line: &str) -> Option<String> {
    if !line.contains("boot Chrome for agent profile") {
        return None;
    }
    // tracing default fmt looks like:
    //   2026-... INFO plugin.browser: boot Chrome for agent profile agent_id=ana ...
    // Find `agent_id=` and capture until the next whitespace.
    let after = line.split("agent_id=").nth(1)?;
    let id: String = after
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    Some(id)
}

fn shutdown_and_wait(
    child: &mut std::process::Child,
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
) {
    let _ = rpc_round_trip(
        stdin,
        stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(unix)] // asserts on the boot log via the Unix-only stderr drain
#[test]
fn distinct_agents_get_distinct_profile_dirs() {
    let (mut child, mut stdin, mut stdout, mut stderr) = spawn_with_env(&[]);
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    let r1 = invoke_press_key(&mut stdin, &mut stdout, 10, Some("ana"));
    assert_eq!(r1["error"]["code"], -33402);

    let r2 = invoke_press_key(&mut stdin, &mut stdout, 11, Some("juan"));
    assert_eq!(r2["error"]["code"], -33402);

    let agents = collect_boot_log_agent_ids(&mut stderr, Duration::from_millis(500));
    assert!(agents.contains(&"ana".to_string()),
        "expected boot log for `ana`, got {:?}", agents);
    assert!(agents.contains(&"juan".to_string()),
        "expected boot log for `juan`, got {:?}", agents);

    shutdown_and_wait(&mut child, &mut stdin, &mut stdout);
}

#[cfg(unix)] // asserts on the boot log via the Unix-only stderr drain
#[test]
fn default_profile_when_agent_id_missing() {
    let (mut child, mut stdin, mut stdout, mut stderr) = spawn_with_env(&[]);
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let r = invoke_press_key(&mut stdin, &mut stdout, 10, None);
    assert_eq!(r["error"]["code"], -33402);

    let agents = collect_boot_log_agent_ids(&mut stderr, Duration::from_millis(500));
    assert!(agents.contains(&"default".to_string()),
        "expected boot log for `default`, got {:?}", agents);

    shutdown_and_wait(&mut child, &mut stdin, &mut stdout);
}

#[test]
fn cap_returns_minus_33404_when_max_profiles_reached() {
    let (mut child, mut stdin, mut stdout, _stderr) =
        spawn_with_env(&[("NEXO_PLUGIN_BROWSER_MAX_PROFILES", "2")]);
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    // First two distinct agents — fill the cap.
    let r1 = invoke_press_key(&mut stdin, &mut stdout, 10, Some("ana"));
    assert_eq!(r1["error"]["code"], -33402); // press-key guard, profile booted.
    let r2 = invoke_press_key(&mut stdin, &mut stdout, 11, Some("juan"));
    assert_eq!(r2["error"]["code"], -33402);

    // Third distinct agent — cap reached, dispatcher returns
    // -33404 BEFORE the press-key guard runs.
    let r3 = invoke_press_key(&mut stdin, &mut stdout, 12, Some("marketing"));
    assert_eq!(r3["error"]["code"], -33404,
        "expected Unavailable (-33404) for cap; got {:?}", r3["error"]);
    let msg = r3["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("max profiles reached"),
        "expected cap message; got `{msg}`");

    shutdown_and_wait(&mut child, &mut stdin, &mut stdout);
}

#[cfg(unix)] // asserts on the boot log via the Unix-only stderr drain
#[test]
fn invalid_agent_id_returns_minus_33402_before_chrome_boot() {
    let (mut child, mut stdin, mut stdout, mut stderr) = spawn_with_env(&[]);
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    let r = invoke_press_key(&mut stdin, &mut stdout, 10, Some("../etc/passwd"));
    assert_eq!(r["error"]["code"], -33402);
    let msg = r["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("agent_id"),
        "expected sanitiser error message about agent_id; got `{msg}`");

    let agents = collect_boot_log_agent_ids(&mut stderr, Duration::from_millis(500));
    assert!(!agents.iter().any(|a| a.contains("etc")),
        "sanitiser must reject BEFORE the boot log fires; got {:?}", agents);

    shutdown_and_wait(&mut child, &mut stdin, &mut stdout);
}

#[cfg(unix)] // asserts on the boot log via the Unix-only stderr drain
#[test]
fn multi_profile_disabled_routes_all_to_default() {
    let (mut child, mut stdin, mut stdout, mut stderr) =
        spawn_with_env(&[("NEXO_PLUGIN_BROWSER_MULTI_PROFILE", "false")]);
    let _init = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );

    let r1 = invoke_press_key(&mut stdin, &mut stdout, 10, Some("ana"));
    assert_eq!(r1["error"]["code"], -33402);
    let r2 = invoke_press_key(&mut stdin, &mut stdout, 11, Some("juan"));
    assert_eq!(r2["error"]["code"], -33402);

    let agents = collect_boot_log_agent_ids(&mut stderr, Duration::from_millis(500));
    // Both calls routed to the same `default` profile — only ONE
    // boot log line should have appeared.
    let default_count = agents.iter().filter(|a| a.as_str() == "default").count();
    assert_eq!(default_count, 1,
        "multi_profile=false: expected exactly one `default` boot log; got {:?}", agents);
    assert!(!agents.iter().any(|a| a == "ana" || a == "juan"),
        "multi_profile=false: per-agent boot logs must NOT appear; got {:?}", agents);

    shutdown_and_wait(&mut child, &mut stdin, &mut stdout);
}
