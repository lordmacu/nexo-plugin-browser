//! Live integration smoke for the Tier 0 (auto-download) path.
//!
//! Brings up `ChromeLauncher::launch` with the env var set and
//! `BrowserConfig.executable` empty, exercising the
//! `chrome-for-testing` integration end-to-end:
//!
//!   1. cargo feature `auto-download` compiled in
//!   2. `NEXO_PLUGIN_BROWSER_AUTO_DOWNLOAD=1` set
//!   3. discovery routes to `try_auto_download`
//!   4. binary downloaded / cache hit
//!   5. chrome-headless-shell spawned with --remote-debugging-port
//!   6. `wait_for_devtools_url` scrapes stderr for the WS URL
//!   7. shutdown reaps cleanly
//!
//! Gated on `NEXO_BROWSER_LIVE_TESTS=1` because the first run
//! downloads ~118 MB and takes ~10s. Subsequent runs hit the
//! cache and finish in <1s.
//!
//! Only available when the plugin was built with `--features
//! auto-download`. Run:
//!
//! ```sh
//! NEXO_BROWSER_LIVE_TESTS=1 cargo test --features auto-download \
//!     --test auto_download -- --ignored --nocapture
//! ```

#![cfg(feature = "auto-download")]

use nexo_plugin_browser::chrome::{ChromeLauncher, RunningChrome};
use nexo_plugin_browser::config::BrowserConfig;

#[tokio::test]
#[ignore]
async fn live_auto_download_launch() {
    if std::env::var_os("NEXO_BROWSER_LIVE_TESTS").is_none() {
        eprintln!("skip: set NEXO_BROWSER_LIVE_TESTS=1 to run");
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,nexo_plugin_browser=debug,chrome_for_testing=debug")
        .try_init();

    // Force the Tier 0 path on for this test, regardless of
    // operator env state at runner-time.
    std::env::set_var("NEXO_PLUGIN_BROWSER_AUTO_DOWNLOAD", "1");

    // Sandbox temp dir so we don't trample the operator's real
    // `./data/browser/profile/` working tree.
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = BrowserConfig {
        instance: None,
        headless: true,
        executable: String::new(), // empty → goes through discovery
        cdp_url: String::new(),
        user_data_dir: tmp.path().to_string_lossy().into_owned(),
        window_width: 1280,
        window_height: 800,
        connect_timeout_ms: 30_000,
        command_timeout_ms: 15_000,
        args: vec!["--no-sandbox".to_string()],
        allow_agents: Vec::new(),
    };

    let running: RunningChrome = ChromeLauncher::launch(&cfg)
        .await
        .expect("chrome-headless-shell auto-download launch");
    eprintln!("ws_url={}", running.ws_url);
    eprintln!("pid={}", running.pid);
    assert!(
        running.ws_url.starts_with("ws://"),
        "expected ws:// scheme on devtools URL, got {}",
        running.ws_url
    );
    running.shutdown().await;
}
