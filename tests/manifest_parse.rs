//! Step 8 of browser-multi-instance — manifest parse coverage.
//!
//! Validates the bundled `nexo-plugin.toml` decodes via
//! `nexo-plugin-manifest::PluginManifest::from_str` and that the
//! Phase 81.33.b.real Stages 1-6 sections are wired correctly.

use nexo_plugin_manifest::dashboard::{AuthCheck, InstanceLayout};
use nexo_plugin_manifest::pairing::PairingKind;
use nexo_plugin_manifest::PluginManifest;

const MANIFEST: &str = include_str!("../nexo-plugin.toml");

fn parse() -> PluginManifest {
    PluginManifest::from_str(MANIFEST).expect("manifest parses")
}

#[test]
fn manifest_parses_with_array_shape() {
    let m = parse();
    let cfg = m
        .plugin
        .config_schema
        .as_ref()
        .expect("config_schema present");
    let shape_dbg = format!("{:?}", cfg.shape).to_lowercase();
    assert!(
        shape_dbg.contains("array"),
        "0.3.0 must declare array shape; got {shape_dbg}"
    );
}

#[test]
fn manifest_pairing_declares_form_with_adapter_and_instance_field() {
    let m = parse();
    let pairing = &m.plugin.pairing;
    assert_eq!(
        pairing.kind,
        Some(PairingKind::Form),
        "browser uses kind=form, not qr"
    );
    assert_eq!(
        pairing.instance_field.as_deref(),
        Some("instance"),
        "instance_field must be `instance`"
    );
    assert!(
        pairing.adapter.is_some(),
        "[plugin.pairing.adapter] required for Stage 1 broker dispatch"
    );
}

#[test]
fn manifest_declares_six_auto_discovery_sections() {
    let m = parse();
    let p = &m.plugin;
    assert!(p.http.is_some(), "[plugin.http] required");
    assert!(p.admin.is_some(), "[plugin.admin] required");
    assert!(p.metrics.is_some(), "[plugin.metrics] required");
    assert!(p.dashboard.is_some(), "[plugin.dashboard.*] required");
    assert_eq!(
        p.pairing.kind,
        Some(PairingKind::Form),
        "[plugin.pairing] required with kind=form"
    );
    assert!(p.config_schema.is_some(), "[plugin.config_schema] required");
}

#[test]
fn manifest_supervisor_respawn_enabled() {
    let m = parse();
    let sup = &m.plugin.supervisor;
    assert!(sup.respawn, "0.3.0 enables per-process auto-respawn");
    assert!(
        sup.max_attempts >= 1,
        "respawn=true must declare max_attempts >= 1; got {}",
        sup.max_attempts
    );
}

#[test]
fn manifest_broker_allowlist_covers_admin_pairing_http_metrics() {
    let m = parse();
    let caps = &m.plugin.capabilities;
    let broker = caps
        .broker
        .as_ref()
        .expect("[plugin.capabilities.broker] required");
    let must_have = [
        "plugin.browser.pairing.normalize_sender",
        "plugin.browser.pairing.send_reply",
        "plugin.browser.pairing.send_qr_image",
        "plugin.browser.http.request",
        "plugin.browser.admin.>",
        "plugin.browser.metrics.scrape",
    ];
    for needed in must_have {
        assert!(
            broker.subscribe.iter().any(|t| t == needed),
            "broker.subscribe missing `{needed}`; declared: {:?}",
            broker.subscribe
        );
    }
}

#[test]
fn manifest_pairing_fields_declare_instance_required_and_initial_url_optional() {
    let m = parse();
    let fields = &m.plugin.pairing.fields;
    let instance = fields
        .iter()
        .find(|f| f.name == "instance")
        .expect("`instance` field");
    assert!(instance.required, "`instance` must be required");
    let url = fields
        .iter()
        .find(|f| f.name == "initial_url")
        .expect("`initial_url` field");
    assert!(!url.required, "`initial_url` must be optional");
}

#[test]
fn manifest_dashboard_workspace_walk_with_paired_sentinel() {
    let m = parse();
    let dash = m.plugin.dashboard.as_ref().unwrap();
    match &dash.layout {
        InstanceLayout::WorkspaceWalk { subdir } => {
            assert_eq!(subdir, "browser", "workspace_walk subdir must be `browser`");
        }
        other => panic!("expected WorkspaceWalk layout, got {other:?}"),
    }
    match &dash.auth_check {
        AuthCheck::SessionDirFiles { candidates } => {
            assert!(
                candidates.iter().any(|c| c == ".nexo-paired"),
                "must include the operator-confirmed sentinel; got {candidates:?}"
            );
        }
        other => panic!("expected SessionDirFiles auth_check, got {other:?}"),
    }
}

#[test]
fn manifest_version_matches_release() {
    let m = parse();
    assert_eq!(m.plugin.version.to_string(), "0.4.2");
}
