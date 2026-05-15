//! Step 10 of browser-multi-instance — Chrome-side cookie
//! isolation between two declared instances.
//!
//! Gated on `CHROMIUM_BIN` env var. Without it the test self-skips
//! (so CI without a browser binary stays green).
//!
//! Strategy (in-process, no subprocess):
//! 1. boot::apply_configure with two instances `a` + `b`.
//! 2. Navigate instance `a` to a data: URL that sets cookie `k=A`.
//! 3. Evaluate `document.cookie` on `a` → contains `k=A`.
//! 4. Evaluate `document.cookie` on `b` → does NOT contain `k=A`.
//!
//! The cookie is set on a `data:` URL host — `document.cookie` is
//! per-origin and `data:` URLs share an opaque origin per Chrome
//! profile but NOT across profiles. Different `user_data_dir`
//! values ⇒ different profiles ⇒ cookie does not bleed.

use nexo_plugin_browser::{boot::apply_configure, configured_state, instance_registry};
use serde_json::json;
use serial_test::serial;

fn yaml(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).expect("yaml parses")
}

async fn reset() {
    *configured_state().write().await = None;
    instance_registry::clear();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
#[ignore = "requires CHROMIUM_BIN; run with `CHROMIUM_BIN=/usr/bin/chromium cargo nextest run --run-ignored=only`"]
async fn cookie_isolation_a_does_not_leak_to_b() {
    if std::env::var("CHROMIUM_BIN").is_err() {
        eprintln!("skipping: CHROMIUM_BIN not set");
        return;
    }
    reset().await;

    let tmp = tempfile::tempdir().expect("tempdir");
    std::env::set_var(
        "NEXO_PLUGIN_BROWSER_USER_DATA_DIR",
        tmp.path().to_str().unwrap(),
    );
    std::env::set_var(
        "NEXO_PLUGIN_BROWSER_EXECUTABLE",
        std::env::var("CHROMIUM_BIN").unwrap(),
    );

    apply_configure(yaml(
        "- instance: a\n  headless: true\n- instance: b\n  headless: true\n",
    ))
    .await
    .expect("configure ok");

    let a = instance_registry::lookup("a").expect("a registered");
    let b = instance_registry::lookup("b").expect("b registered");

    // Navigate `a` to a page that sets a cookie.
    let nav_a = nexo_plugin_browser::dispatch::dispatch_browser_tool(
        a.clone(),
        "browser_navigate",
        json!({
            "url": "data:text/html,<script>document.cookie='k=A;path=/';</script>OK"
        }),
    )
    .await
    .expect("navigate a ok");
    assert_eq!(nav_a["ok"].as_bool(), Some(true));

    // Read cookie on `a`.
    let cookie_a = nexo_plugin_browser::dispatch::dispatch_browser_tool(
        a.clone(),
        "browser_evaluate",
        json!({ "script": "document.cookie" }),
    )
    .await
    .expect("evaluate a ok");
    let cookie_a_str = cookie_a["result"].as_str().unwrap_or("");
    assert!(
        cookie_a_str.contains("k=A"),
        "cookie k=A must be visible on instance `a`; got: {cookie_a_str}"
    );

    // Navigate `b` to a benign data: URL so document.cookie has a
    // value to read against.
    let nav_b = nexo_plugin_browser::dispatch::dispatch_browser_tool(
        b.clone(),
        "browser_navigate",
        json!({ "url": "data:text/html,OK" }),
    )
    .await
    .expect("navigate b ok");
    assert_eq!(nav_b["ok"].as_bool(), Some(true));

    let cookie_b = nexo_plugin_browser::dispatch::dispatch_browser_tool(
        b.clone(),
        "browser_evaluate",
        json!({ "script": "document.cookie" }),
    )
    .await
    .expect("evaluate b ok");
    let cookie_b_str = cookie_b["result"].as_str().unwrap_or("");
    assert!(
        !cookie_b_str.contains("k=A"),
        "cookie k=A must NOT bleed into instance `b`; got: {cookie_b_str}"
    );

    // Tear down Chrome processes both instances launched.
    a.shutdown_chrome().await;
    b.shutdown_chrome().await;
    reset().await;
    std::env::remove_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR");
    std::env::remove_var("NEXO_PLUGIN_BROWSER_EXECUTABLE");
}
