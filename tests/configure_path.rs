//! Phase 93.4.e — coverage for the configure(value) hook +
//! configured_state singleton.

use nexo_plugin_browser::{configured_state, BrowserConfig};
use serial_test::serial;

fn parse_yaml(s: &str) -> BrowserConfig {
    let value: serde_yaml::Value = serde_yaml::from_str(s).expect("yaml parses");
    serde_yaml::from_value(value).expect("config deserialises")
}

#[tokio::test]
#[serial]
async fn configure_deserialises_object_shape() {
    let cfg = parse_yaml("headless: true\nwindow_width: 1920\n");
    assert!(cfg.headless);
    assert_eq!(cfg.window_width, 1920);
    // Defaults applied for omitted fields.
    assert_eq!(cfg.window_height, 800);
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn configure_unknown_field_errors() {
    let value: serde_yaml::Value = serde_yaml::from_str("bogus_field: 1\n").unwrap();
    let res: Result<BrowserConfig, _> = serde_yaml::from_value(value);
    let err = res.expect_err("unknown field must fail");
    assert!(
        err.to_string().to_lowercase().contains("bogus_field"),
        "error should mention bogus_field, got: {err}",
    );
}

#[tokio::test]
#[serial]
async fn configure_overwrites_on_hot_reload_recall() {
    let cfg_a = parse_yaml("user_data_dir: /tmp/a\n");
    *configured_state().write().await = Some(cfg_a);

    let cfg_b = parse_yaml("user_data_dir: /tmp/b\n");
    *configured_state().write().await = Some(cfg_b);

    let guard = configured_state().read().await;
    let current = guard.as_ref().expect("populated");
    assert_eq!(current.user_data_dir, "/tmp/b");
    drop(guard);
    *configured_state().write().await = None;
}

#[tokio::test]
#[serial]
async fn configure_args_array_preserved() {
    let cfg = parse_yaml(
        "args:\n  - \"--no-sandbox\"\n  - \"--disable-dev-shm-usage\"\n",
    );
    assert_eq!(cfg.args.len(), 2);
    assert_eq!(cfg.args[0], "--no-sandbox");
}

#[tokio::test]
#[serial]
async fn configured_state_holds_single_instance_not_vec() {
    let cfg = parse_yaml("headless: true\n");
    *configured_state().write().await = Some(cfg);
    let guard = configured_state().read().await;
    let inner = guard.as_ref().expect("populated");
    assert!(inner.headless);
    drop(guard);
    *configured_state().write().await = None;
}
