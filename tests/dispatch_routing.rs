//! Phase browser-multi-instance Step 5 — coverage for
//! `resolve_plugin_or_legacy` across the four resolution cases.

use std::sync::Arc;

use nexo_plugin_browser::{
    config::BrowserConfig, dispatch::resolve_plugin_or_legacy, instance_registry,
    BrowserPlugin,
};
use serde_json::json;
use serial_test::serial;

fn make_plugin(instance: Option<&str>, allow: &[&str]) -> Arc<BrowserPlugin> {
    Arc::new(BrowserPlugin::new(BrowserConfig {
        instance: instance.map(|s| s.to_string()),
        headless: true,
        executable: String::new(),
        cdp_url: String::new(),
        user_data_dir: format!(
            "./.test-routing-{}",
            instance.unwrap_or("legacy")
        ),
        window_width: 1280,
        window_height: 800,
        connect_timeout_ms: 8_000,
        command_timeout_ms: 30_000,
        args: Vec::new(),
        allow_agents: allow.iter().map(|s| s.to_string()).collect(),
    }))
}

#[test]
#[serial]
fn case1_explicit_instance_matches_registered() {
    instance_registry::clear();
    let alpha = make_plugin(Some("alpha"), &[]);
    instance_registry::register("alpha", alpha.clone());

    let legacy = make_plugin(None, &[]);
    let plugin = resolve_plugin_or_legacy(
        &json!({ "instance": "alpha" }),
        "ana",
        legacy.clone(),
    )
    .expect("registered instance resolves");
    assert!(Arc::ptr_eq(&plugin, &alpha));
    assert!(!Arc::ptr_eq(&plugin, &legacy));
    instance_registry::clear();
}

#[test]
#[serial]
fn case1_explicit_instance_unknown_returns_argument_invalid() {
    instance_registry::clear();
    let legacy = make_plugin(None, &[]);
    let err = resolve_plugin_or_legacy(
        &json!({ "instance": "ghost" }),
        "ana",
        legacy,
    )
    .err().expect("unknown instance must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("ghost"), "error must echo the bad name: {msg}");
}

#[test]
#[serial]
fn case2_implicit_single_declared_uses_it() {
    instance_registry::clear();
    let only = make_plugin(Some("only"), &[]);
    instance_registry::register("only", only.clone());

    let legacy = make_plugin(None, &[]);
    let plugin =
        resolve_plugin_or_legacy(&json!({}), "ana", legacy.clone()).expect("ok");
    assert!(Arc::ptr_eq(&plugin, &only));
    instance_registry::clear();
}

#[test]
#[serial]
fn case3_implicit_zero_declared_falls_back_to_legacy() {
    instance_registry::clear();
    let legacy = make_plugin(None, &[]);
    let plugin =
        resolve_plugin_or_legacy(&json!({}), "ana", legacy.clone()).expect("ok");
    assert!(Arc::ptr_eq(&plugin, &legacy));
}

#[test]
#[serial]
fn case4_implicit_multi_declared_returns_ambiguous() {
    instance_registry::clear();
    instance_registry::register("a", make_plugin(Some("a"), &[]));
    instance_registry::register("b", make_plugin(Some("b"), &[]));

    let legacy = make_plugin(None, &[]);
    let err = resolve_plugin_or_legacy(&json!({}), "ana", legacy)
        .err()
        .expect("multi-instance without arg must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("multiple instances") || msg.contains("instance"),
        "error must mention ambiguity: {msg}"
    );
    instance_registry::clear();
}

#[test]
#[serial]
fn allow_agents_gate_rejects_unlisted_agent() {
    instance_registry::clear();
    let gated = make_plugin(Some("gated"), &["alice", "bob"]);
    instance_registry::register("gated", gated);

    let legacy = make_plugin(None, &[]);
    let err = resolve_plugin_or_legacy(
        &json!({ "instance": "gated" }),
        "eve",
        legacy,
    )
    .err().expect("agent not in allow_agents must fail");
    let msg = format!("{err:?}");
    assert!(msg.contains("eve"), "error must echo agent: {msg}");
    assert!(
        msg.contains("allow_agents") || msg.contains("not in"),
        "error must mention the gate: {msg}"
    );
    instance_registry::clear();
}

#[test]
#[serial]
fn allow_agents_gate_admits_listed_agent() {
    instance_registry::clear();
    let gated = make_plugin(Some("gated"), &["alice", "bob"]);
    instance_registry::register("gated", gated.clone());

    let legacy = make_plugin(None, &[]);
    let plugin = resolve_plugin_or_legacy(
        &json!({ "instance": "gated" }),
        "alice",
        legacy,
    )
    .expect("alice admitted");
    assert!(Arc::ptr_eq(&plugin, &gated));
    instance_registry::clear();
}

#[test]
#[serial]
fn allow_agents_empty_admits_anyone() {
    instance_registry::clear();
    let open = make_plugin(Some("open"), &[]);
    instance_registry::register("open", open.clone());

    let legacy = make_plugin(None, &[]);
    let plugin = resolve_plugin_or_legacy(
        &json!({ "instance": "open" }),
        "random-agent",
        legacy,
    )
    .expect("empty allow_agents accepts anyone");
    assert!(Arc::ptr_eq(&plugin, &open));
    instance_registry::clear();
}
