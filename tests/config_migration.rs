//! Phase 81.x browser-multi-instance Step 1 — coverage for the
//! `BrowserPluginShape` enum + back-compat with 0.2.x bare-object
//! YAML.

use nexo_plugin_browser::{BrowserPluginConfigFile, BrowserPluginShape};

fn parse(s: &str) -> BrowserPluginConfigFile {
    serde_yaml::from_str(s).expect("yaml parses")
}

#[test]
fn single_object_form_back_compat() {
    let parsed = parse(
        "browser:\n  headless: true\n  window_width: 1920\n",
    );
    match &parsed.browser {
        BrowserPluginShape::Single(c) => {
            assert!(c.headless);
            assert_eq!(c.window_width, 1920);
            assert!(c.instance.is_none());
        }
        BrowserPluginShape::Many(_) => panic!("expected Single, got Many"),
    }
    let v = parsed.browser.into_vec();
    assert_eq!(v.len(), 1);
}

#[test]
fn array_form_parses_multi() {
    let parsed = parse(
        "browser:\n  - instance: a\n    headless: true\n  - instance: b\n    window_width: 1024\n",
    );
    let v = parsed.browser.into_vec();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].instance.as_deref(), Some("a"));
    assert!(v[0].headless);
    assert_eq!(v[1].instance.as_deref(), Some("b"));
    assert_eq!(v[1].window_width, 1024);
}

#[test]
fn array_form_single_element_ok() {
    let parsed = parse("browser:\n  - instance: solo\n");
    let v = parsed.browser.into_vec();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].instance.as_deref(), Some("solo"));
}

#[test]
fn unknown_field_rejected_in_single() {
    // Untagged enum: `serde` tries Single then Many, both fail with
    // `deny_unknown_fields`, so the surface error is the generic
    // "did not match any variant" — sufficient signal that the
    // bogus field made every variant reject the input.
    let res: Result<BrowserPluginConfigFile, _> =
        serde_yaml::from_str("browser:\n  bogus_field: 1\n");
    res.expect_err("unknown field must fail");
}

#[test]
fn unknown_field_rejected_in_array_entry() {
    let res: Result<BrowserPluginConfigFile, _> =
        serde_yaml::from_str("browser:\n  - bogus_field: 1\n");
    res.expect_err("unknown field must fail in array entry");
}

#[test]
fn allow_agents_field_parses() {
    let parsed = parse(
        "browser:\n  - instance: marketing\n    allow_agents: [ana, juan]\n",
    );
    let v = parsed.browser.into_vec();
    assert_eq!(v[0].allow_agents, vec!["ana", "juan"]);
}

#[test]
fn empty_user_data_dir_default_string_via_serde() {
    // Default string from `#[serde(default = "default_user_data_dir")]`
    // — kept stable for the legacy single-instance path. Step 6 will
    // override to the auto-resolved `${state_dir}/instances/<label>/`
    // when this default is detected.
    let parsed = parse("browser:\n  - instance: x\n");
    let v = parsed.browser.into_vec();
    assert_eq!(v[0].user_data_dir, "./data/browser/profile");
}
