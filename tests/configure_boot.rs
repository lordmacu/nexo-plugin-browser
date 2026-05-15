//! Step 6 of browser-multi-instance — coverage for the
//! `boot::apply_configure` boot loop.

use nexo_plugin_browser::{boot::apply_configure, configured_state, instance_registry};
use serial_test::serial;

fn yaml(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).expect("yaml parses")
}

async fn reset() {
    *configured_state().write().await = None;
    instance_registry::clear();
}

#[tokio::test]
#[serial]
async fn configure_with_two_instances_registers_both() {
    reset().await;
    let v = yaml(
        "- instance: alpha\n  headless: true\n- instance: beta\n  window_width: 1024\n",
    );
    apply_configure(v).await.expect("configure ok");

    assert_eq!(instance_registry::len(), 2);
    assert!(instance_registry::lookup("alpha").is_some());
    assert!(instance_registry::lookup("beta").is_some());

    let guard = configured_state().read().await;
    let snap = guard.as_ref().expect("populated");
    assert_eq!(snap.len(), 2);
    drop(guard);
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_reload_drops_removed_instance() {
    reset().await;
    apply_configure(yaml(
        "- instance: a\n- instance: b\n- instance: c\n",
    ))
    .await
    .expect("first configure");
    assert_eq!(instance_registry::len(), 3);

    apply_configure(yaml("- instance: a\n- instance: c\n"))
        .await
        .expect("reload");
    assert_eq!(instance_registry::len(), 2);
    assert!(instance_registry::lookup("a").is_some());
    assert!(instance_registry::lookup("b").is_none());
    assert!(instance_registry::lookup("c").is_some());
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_duplicate_label_errors() {
    reset().await;
    let err = apply_configure(yaml(
        "- instance: dup\n- instance: dup\n",
    ))
    .await
    .err()
    .expect("duplicate must fail");
    assert!(
        err.to_lowercase().contains("duplicate"),
        "error must mention duplicate: {err}"
    );
    // Failed configure must not leave half-populated registry.
    assert_eq!(instance_registry::len(), 0);
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_empty_user_data_dir_resolves_to_default_path() {
    reset().await;
    std::env::set_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR", "/tmp/test-root");

    apply_configure(yaml(
        "- instance: marketing\n  user_data_dir: \"\"\n",
    ))
    .await
    .expect("configure ok");

    let plugin = instance_registry::lookup("marketing").expect("registered");
    assert_eq!(plugin.user_data_dir(), "/tmp/test-root/instances/marketing");

    std::env::remove_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR");
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_default_user_data_dir_sentinel_also_resolves() {
    reset().await;
    std::env::set_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR", "/tmp/test-root2");

    // The serde default for `user_data_dir` is `./data/browser/profile`.
    // The boot loop detects that sentinel and replaces it too so legacy
    // 0.2.x defaults don't leak into multi-instance setups.
    apply_configure(yaml(
        "- instance: x\n  user_data_dir: ./data/browser/profile\n",
    ))
    .await
    .expect("configure ok");

    let plugin = instance_registry::lookup("x").expect("registered");
    assert_eq!(plugin.user_data_dir(), "/tmp/test-root2/instances/x");

    std::env::remove_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR");
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_legacy_single_object_back_compat_skips_registry() {
    reset().await;
    // Single-map shape ⇒ legacy per-agent_id path. Registry stays
    // empty; only `configured_state` is populated.
    apply_configure(yaml("headless: true\nwindow_width: 1280\n"))
        .await
        .expect("configure ok");

    assert_eq!(instance_registry::len(), 0);
    let guard = configured_state().read().await;
    let snap = guard.as_ref().expect("populated");
    assert_eq!(snap.len(), 1);
    assert!(snap[0].headless);
    drop(guard);
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_invalid_label_errors() {
    reset().await;
    let err = apply_configure(yaml("- instance: \"bad/label\"\n"))
        .await
        .err()
        .expect("invalid label must fail");
    assert!(
        err.to_lowercase().contains("invalid"),
        "error must mention invalid: {err}"
    );
    assert_eq!(instance_registry::len(), 0);
    reset().await;
}

#[tokio::test]
#[serial]
async fn configure_resolved_instance_label_written_back() {
    reset().await;
    apply_configure(yaml(
        "- instance: Marketing\n", // Mixed case — sanitiser lowercases.
    ))
    .await
    .expect("ok");

    let plugin = instance_registry::lookup("marketing").expect("lowercased");
    assert_eq!(
        plugin.config_snapshot().instance.as_deref(),
        Some("marketing"),
        "boot loop must write the resolved label back into BrowserConfig.instance"
    );
    reset().await;
}
