use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::config::BrowserConfig;

// Discovery logic lives in its own module so the per-platform
// candidate lists can grow and be tested independently.
mod discovery;
use discovery::find_chrome_executable;

/// Categorisation of a discovered browser. Drives kind-specific
/// quirks at launch time (e.g. Edge in some builds requires
/// `--remote-allow-origins=*` for CDP). The taxonomy is pruned
/// to the kinds we actually launch via CDP — derivative browsers
/// (Brave, Vivaldi, Opera) are intentionally excluded from
/// auto-detect; operators run them via the explicit
/// `NEXO_PLUGIN_BROWSER_EXECUTABLE` override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserKind {
    Chrome,
    Chromium,
    /// `msedge` — Chromium-based, CDP-compatible. Common on
    /// Windows enterprise installs where Chrome may be absent.
    Edge,
    /// Operator-supplied path via
    /// `NEXO_PLUGIN_BROWSER_EXECUTABLE`. We cannot infer kind
    /// from a custom path without sniffing the binary, so we
    /// keep it opaque.
    Custom,
}

/// A successfully discovered browser executable. The `kind` is
/// informational (logged on launch); `path` is the absolute
/// filesystem path the launcher will spawn.
#[derive(Debug, Clone)]
pub struct BrowserExecutable {
    pub kind: BrowserKind,
    pub path: PathBuf,
}

/// Errors produced by the discovery layer. Public so future
/// integration tests can match on the shape; `launch()` wraps
/// these in `anyhow::Error` for the operator-facing path.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// Auto-detect exhausted every Tier 1 (bundled candidates)
    /// and Tier 2 (PATH lookup) check without finding a usable
    /// browser. `searched` lists every concrete path the
    /// resolver tested so the operator can copy-paste it into a
    /// bug report or set `NEXO_PLUGIN_BROWSER_EXECUTABLE`.
    #[error(
        "no Chrome/Chromium/Edge executable found; searched {} paths; \
         set NEXO_PLUGIN_BROWSER_EXECUTABLE to override",
        searched.len()
    )]
    NotFound { searched: Vec<String> },

    /// `NEXO_PLUGIN_BROWSER_EXECUTABLE` was set but points at a
    /// path that doesn't exist. We fail-fast here rather than
    /// silently fall back to auto-detect — explicit override
    /// implies strong intent; a typo masked by silent fallback
    /// is harder to diagnose than a clear error.
    #[error("NEXO_PLUGIN_BROWSER_EXECUTABLE points to non-existent path: {path}")]
    OverrideMissing { path: String },
}

pub struct RunningChrome {
    pub ws_url: String,
    pub pid: u32,
    child: tokio::process::Child,
}

impl RunningChrome {
    /// Kill Chrome and reap the process entry.
    /// Must be called from an async context before the value goes out of scope.
    /// Consuming self ensures the Child is dropped only after wait() completes,
    /// preventing a <defunct> entry in the process table.
    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

impl Drop for RunningChrome {
    fn drop(&mut self) {
        // Safety-net: reached only on unexpected drops (panic path, test teardown).
        // Normal teardown must go through shutdown() to avoid zombie processes.
        tracing::warn!(
            pid = self.pid,
            "RunningChrome dropped without shutdown() — Chrome process may become a zombie"
        );
        let _ = self.child.start_kill();
    }
}

pub struct ChromeLauncher;

impl ChromeLauncher {
    pub async fn launch(config: &BrowserConfig) -> anyhow::Result<RunningChrome> {
        // Resolve the browser via the two-tier (bundled candidates
        // + PATH) discovery in `chrome::discovery`. The override
        // branch wraps the operator-supplied path in a `Custom`
        // kind so the log line below is deterministic regardless
        // of which path the caller took.
        let (resolved, source): (BrowserExecutable, &'static str) = if config.executable.is_empty()
        {
            // Auto-detect via the two-tier resolver. On `Err` the
            // resolver hands back the full searched-paths list so
            // the operator's actionable hint quotes every probe
            // site.
            let exe = find_chrome_executable().map_err(|e| match &e {
                DiscoveryError::NotFound { searched } => anyhow!(
                    "no Chrome/Chromium/Edge executable found — searched {} location(s):\n  {}\nset \
                     NEXO_PLUGIN_BROWSER_EXECUTABLE to an absolute path to override",
                    searched.len(),
                    searched.join("\n  "),
                ),
                // `OverrideMissing` only originates from the
                // override branch below; the auto-detect arm
                // can't produce it.
                DiscoveryError::OverrideMissing { .. } => anyhow!(e.to_string()),
            })?;
            (exe, "auto-detect")
        } else {
            // Fail-fast when an explicit override points at a
            // missing path. Otherwise we'd fall through to spawn
            // and the operator would see a generic
            // "failed to spawn Chrome (...)" with the real cause
            // (NotFound) buried in the OS error chain. An explicit
            // override implies strong intent — a typo is far more
            // likely than the operator wanting an implicit
            // fallback to auto-detect.
            let override_path = PathBuf::from(&config.executable);
            if !override_path.exists() {
                anyhow::bail!(DiscoveryError::OverrideMissing {
                    path: config.executable.clone(),
                });
            }
            (
                BrowserExecutable {
                    kind: BrowserKind::Custom,
                    path: override_path,
                },
                "env-override",
            )
        };
        tracing::info!(
            target: "browser.discovery",
            kind = ?resolved.kind,
            path = %resolved.path.display(),
            source = source,
            "Chrome/Chromium/Edge executable resolved"
        );
        let exe = &resolved.path;

        let mut args = vec![
            format!("--remote-debugging-port=0"),
            format!("--user-data-dir={}", config.user_data_dir),
            format!(
                "--window-size={},{}",
                config.window_width, config.window_height
            ),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-background-networking".to_string(),
            "--disable-client-side-phishing-detection".to_string(),
            "--disable-default-apps".to_string(),
            "--disable-extensions".to_string(),
            "--disable-hang-monitor".to_string(),
            "--disable-popup-blocking".to_string(),
            "--disable-prompt-on-repost".to_string(),
            "--disable-sync".to_string(),
            "--disable-translate".to_string(),
            "--metrics-recording-only".to_string(),
            "--safebrowsing-disable-auto-update".to_string(),
            "about:blank".to_string(),
        ];

        if config.headless {
            args.push("--headless=new".to_string());
            args.push("--disable-gpu".to_string());
        }

        // Operator-supplied extra flags (`browser.args` in YAML). Appended
        // after our built-ins so the operator can override if a flag
        // conflicts — later args win with Chrome's CLI parser.
        for extra in &config.args {
            args.push(extra.clone());
        }

        let mut child = Command::new(&exe)
            .args(&args)
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("failed to spawn Chrome ({}): {e}", exe.display()))?;

        let pid = child.id().unwrap_or(0);

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture Chrome stderr"))?;

        let ws_url = wait_for_devtools_url(stderr, config.connect_timeout_ms)
            .await
            .inspect_err(|_e| {
                let _ = child.start_kill();
            })?;

        Ok(RunningChrome { ws_url, pid, child })
    }

    /// Attach to an already-running Chrome at a CDP HTTP URL.
    pub async fn connect_existing(cdp_url: &str, timeout_ms: u64) -> anyhow::Result<String> {
        // Delegate to the canonical discovery path in cdp::client which also
        // rewrites the authority of the returned webSocketDebuggerUrl
        // (Chrome echoes the Host header and drops the real port).
        crate::cdp::CdpClient::discover_ws_url(cdp_url, timeout_ms).await
    }
}

async fn wait_for_devtools_url(
    stderr: tokio::process::ChildStderr,
    timeout_ms: u64,
) -> anyhow::Result<String> {
    let reader = tokio::io::BufReader::new(stderr);
    let mut lines = reader.lines();

    let timeout = Duration::from_millis(timeout_ms);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or(Duration::ZERO);

        if remaining.is_zero() {
            return Err(anyhow!("timed out waiting for Chrome DevTools URL"));
        }

        let line = tokio::time::timeout(remaining, lines.next_line())
            .await
            .map_err(|_| anyhow!("timed out waiting for Chrome DevTools URL"))?
            .map_err(|e| anyhow!("error reading Chrome stderr: {e}"))?;

        let Some(line) = line else {
            return Err(anyhow!("Chrome exited before DevTools URL appeared"));
        };

        if let Some(rest) = line.strip_prefix("DevTools listening on ") {
            return Ok(rest.trim().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)] // only the Unix-only `shutdown_reaps_process` test uses it
    use tokio::process::Command as TokioCommand;

    #[test]
    fn browser_kind_debug_shape() {
        // Smoke check that the public Debug projection lines up
        // with what we log on launch — operators grep this.
        assert_eq!(format!("{:?}", BrowserKind::Chrome), "Chrome");
        assert_eq!(format!("{:?}", BrowserKind::Chromium), "Chromium");
        assert_eq!(format!("{:?}", BrowserKind::Edge), "Edge");
        assert_eq!(format!("{:?}", BrowserKind::Custom), "Custom");
    }

    #[test]
    fn browser_executable_round_trips_through_clone() {
        let exe = BrowserExecutable {
            kind: BrowserKind::Chrome,
            path: PathBuf::from("/usr/bin/google-chrome"),
        };
        let cloned = exe.clone();
        assert_eq!(cloned.kind, BrowserKind::Chrome);
        assert_eq!(cloned.path, exe.path);
    }

    #[test]
    fn discovery_error_not_found_displays_searched_count() {
        let err = DiscoveryError::NotFound {
            searched: vec!["/a".into(), "/b".into(), "/c".into()],
        };
        let msg = format!("{err}");
        assert!(msg.contains("3 paths"), "got: {msg}");
        assert!(msg.contains("NEXO_PLUGIN_BROWSER_EXECUTABLE"), "got: {msg}");
    }

    #[test]
    fn discovery_error_override_missing_includes_path() {
        let err = DiscoveryError::OverrideMissing {
            path: "/nonexistent/chrome.exe".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/nonexistent/chrome.exe"), "got: {msg}");
    }

    /// Minimal `BrowserConfig` with `executable` field set;
    /// other fields populated with the same defaults the YAML
    /// loader applies. Local helper so the override-missing
    /// test below doesn't need to deserialize a TOML/YAML stub.
    fn config_with_executable(exe: &str) -> BrowserConfig {
        BrowserConfig {
            headless: false,
            executable: exe.to_string(),
            cdp_url: String::new(),
            user_data_dir: "./data/browser/profile".to_string(),
            window_width: 1280,
            window_height: 800,
            connect_timeout_ms: 10_000,
            command_timeout_ms: 15_000,
            args: Vec::new(),
        }
    }

    #[tokio::test]
    async fn launch_fails_fast_when_override_path_missing() {
        // `NEXO_PLUGIN_BROWSER_EXECUTABLE` (surface form: the
        // `executable` field on `BrowserConfig`) was set to a
        // path that doesn't exist. We must surface the typed
        // `DiscoveryError::OverrideMissing` so the operator sees
        // the typo immediately rather than an OS NotFound buried
        // inside a generic spawn failure.
        let cfg = config_with_executable("/definitely-not-a-real/chrome-binary-xyz");
        // `RunningChrome` doesn't derive `Debug` (holds a
        // `tokio::process::Child`), so `.expect_err` won't
        // compile. Match instead.
        let err = match ChromeLauncher::launch(&cfg).await {
            Ok(_) => panic!("launch must fail-fast on missing override"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("/definitely-not-a-real/chrome-binary-xyz"),
            "error must echo the bad path: {msg}"
        );
        assert!(
            msg.contains("non-existent path"),
            "error must label the failure mode: {msg}"
        );
    }

    #[cfg(unix)] // only used by the Unix-only `shutdown_reaps_process` test
    fn make_running_chrome(child: tokio::process::Child, pid: u32) -> RunningChrome {
        RunningChrome {
            ws_url: String::new(),
            pid,
            child,
        }
    }

    // Unix-only: relies on the `sleep` binary + `libc::kill(pid, 0)`
    // ESRCH probe to verify process reaping. Windows uses different
    // process-management primitives (`OpenProcess` / `WaitForSingleObject`)
    // and ships no `sleep`. The reaping logic in `RunningChrome::shutdown`
    // itself is portable (uses `tokio::process::Child::wait`); only the
    // test harness is Unix-specific.
    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_reaps_process() {
        let child = TokioCommand::new("sleep")
            .arg("30")
            .kill_on_drop(false)
            .spawn()
            .expect("failed to spawn sleep");
        let pid = child.id().expect("missing PID");
        let running = make_running_chrome(child, pid);

        running.shutdown().await;

        // After shutdown(), the process must be reaped.
        // kill(pid, 0) returns ESRCH when no such process exists.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
        assert!(!alive, "process {pid} still exists after shutdown()");
    }
}
