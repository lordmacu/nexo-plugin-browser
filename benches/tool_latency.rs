//! Subprocess `tool.invoke` round-trip latency benchmark.
//!
//! Plain `Instant::now()`-based loop instead of criterion to keep
//! the dep graph thin; the goal is operator-observable numbers
//! for the README, not statistical analysis.
//!
//! Run with `cargo bench --bench tool_latency`. Without
//! `CHROMIUM_BIN` set, only the pre-Chrome dispatch overhead
//! benchmark runs (instant; no Chrome boot).
//!
//! Numbers are environment-dependent — operator paste into
//! README's Latency budget table per their hardware.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BINARY: &str = env!("CARGO_BIN_EXE_nexo-plugin-browser");
const ITERATIONS: usize = 200;

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

fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64) * p).clamp(0.0, (samples.len() - 1) as f64) as usize;
    samples[idx]
}

fn report(label: &str, samples: &mut Vec<Duration>) {
    let avg: Duration = samples.iter().sum::<Duration>() / (samples.len() as u32);
    let p95 = percentile(samples, 0.95);
    let p99 = percentile(samples, 0.99);
    println!(
        "  {label:32}  n={n:<4}  avg={avg:>10.3?}  p95={p95:>10.3?}  p99={p99:>10.3?}",
        n = samples.len(),
    );
}

fn bench_dispatch_overhead_pre_chrome() {
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

    let mut samples = Vec::with_capacity(ITERATIONS);
    for i in 0..ITERATIONS {
        let frame = json!({
            "jsonrpc": "2.0",
            "id": 100 + i,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_press_key",
                "args": { "key": "" },
            },
        });
        let t = Instant::now();
        let _r = rpc_round_trip(&mut stdin, &mut stdout, frame);
        samples.push(t.elapsed());
    }
    report("browser_press_key (rejected)", &mut samples);

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = wait_timeout_or_kill(&mut child, Duration::from_secs(2));
}

fn bench_browser_navigate_with_chromium() -> bool {
    let Ok(chromium) = std::env::var("CHROMIUM_BIN") else {
        println!("  (skipped — CHROMIUM_BIN unset)");
        return false;
    };
    std::env::set_var("NEXO_PLUGIN_BROWSER_EXECUTABLE", chromium);
    std::env::set_var("NEXO_PLUGIN_BROWSER_HEADLESS", "true");

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

    // Warm-up: first call boots Chromium (slow). Measure steady
    // state, not cold start.
    let _warm = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_navigate",
                "args": { "url": "about:blank" },
            },
        }),
    );

    let mut samples = Vec::with_capacity(50);
    for i in 0..50 {
        let frame = json!({
            "jsonrpc": "2.0",
            "id": 100 + i,
            "method": "tool.invoke",
            "params": {
                "plugin_id": "browser",
                "tool_name": "browser_current_url",
                "args": {},
            },
        });
        let t = Instant::now();
        let _r = rpc_round_trip(&mut stdin, &mut stdout, frame);
        samples.push(t.elapsed());
    }
    report("browser_current_url (live CDP)", &mut samples);

    let _ = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}),
    );
    let _ = wait_timeout_or_kill(&mut child, Duration::from_secs(5));
    true
}

fn wait_timeout_or_kill(
    child: &mut std::process::Child,
    dur: Duration,
) -> std::io::Result<()> {
    let deadline = Instant::now() + dur;
    loop {
        match child.try_wait()? {
            Some(_) => return Ok(()),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                return child.wait().map(|_| ());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn main() {
    println!();
    println!("nexo-plugin-browser :: tool.invoke round-trip latency");
    println!("====================================================");
    println!();
    println!("# Pre-Chrome path (pure SDK dispatch loop overhead)");
    bench_dispatch_overhead_pre_chrome();
    println!();
    println!("# Live Chromium path (full CDP roundtrip)");
    bench_browser_navigate_with_chromium();
    println!();
}
