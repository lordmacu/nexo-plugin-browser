//! Build a [`nexo_config::BrowserConfig`] from environment
//! variables. The daemon translates its `cfg.plugins.browser`
//! YAML into env vars before spawning the subprocess; this module
//! is the child-side reader.
//!
//! Convention: every knob has a `NEXO_PLUGIN_BROWSER_*` name.
//! Defaults match the YAML loader's `#[serde(default)]` values so
//! a missing env var behaves the same as an absent YAML key.

use crate::config::BrowserConfig;

/// Read the browser config from `NEXO_PLUGIN_BROWSER_*` env vars.
/// Missing vars fall back to the same defaults the YAML loader
/// applies via `#[serde(default)]`.
pub fn browser_config_from_env() -> BrowserConfig {
    BrowserConfig {
        instance: None,
        headless: parse_bool("NEXO_PLUGIN_BROWSER_HEADLESS").unwrap_or(false),
        executable: std::env::var("NEXO_PLUGIN_BROWSER_EXECUTABLE").unwrap_or_default(),
        cdp_url: std::env::var("NEXO_PLUGIN_BROWSER_CDP_URL").unwrap_or_default(),
        user_data_dir: std::env::var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR")
            .unwrap_or_else(|_| "./.browser-profile".to_string()),
        window_width: parse_u32("NEXO_PLUGIN_BROWSER_WINDOW_WIDTH").unwrap_or(1280),
        window_height: parse_u32("NEXO_PLUGIN_BROWSER_WINDOW_HEIGHT").unwrap_or(800),
        connect_timeout_ms: parse_u64("NEXO_PLUGIN_BROWSER_CONNECT_TIMEOUT_MS").unwrap_or(8_000),
        command_timeout_ms: parse_u64("NEXO_PLUGIN_BROWSER_COMMAND_TIMEOUT_MS").unwrap_or(30_000),
        // Comma-separated list, semicolons also accepted. Empty
        // string => empty Vec.
        args: parse_args_csv("NEXO_PLUGIN_BROWSER_ARGS"),
        allow_agents: Vec::new(),
    }
}

fn parse_args_csv(name: &str) -> Vec<String> {
    let Ok(raw) = std::env::var(name) else {
        return Vec::new();
    };
    raw.split(|c: char| c == ',' || c == ';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_bool(name: &str) -> Option<bool> {
    let v = std::env::var(name).ok()?;
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}

fn parse_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn clear_env() {
        for k in [
            "NEXO_PLUGIN_BROWSER_HEADLESS",
            "NEXO_PLUGIN_BROWSER_EXECUTABLE",
            "NEXO_PLUGIN_BROWSER_CDP_URL",
            "NEXO_PLUGIN_BROWSER_USER_DATA_DIR",
            "NEXO_PLUGIN_BROWSER_WINDOW_WIDTH",
            "NEXO_PLUGIN_BROWSER_WINDOW_HEIGHT",
            "NEXO_PLUGIN_BROWSER_CONNECT_TIMEOUT_MS",
            "NEXO_PLUGIN_BROWSER_COMMAND_TIMEOUT_MS",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    #[serial]
    fn defaults_when_no_env() {
        clear_env();
        let cfg = browser_config_from_env();
        assert!(!cfg.headless);
        assert_eq!(cfg.executable, "");
        assert_eq!(cfg.cdp_url, "");
        assert_eq!(cfg.user_data_dir, "./.browser-profile");
        assert_eq!(cfg.window_width, 1280);
        assert_eq!(cfg.window_height, 800);
        assert_eq!(cfg.connect_timeout_ms, 8_000);
        assert_eq!(cfg.command_timeout_ms, 30_000);
    }

    #[test]
    #[serial]
    fn parses_headless_true_false() {
        clear_env();
        std::env::set_var("NEXO_PLUGIN_BROWSER_HEADLESS", "true");
        assert!(browser_config_from_env().headless);
        std::env::set_var("NEXO_PLUGIN_BROWSER_HEADLESS", "0");
        assert!(!browser_config_from_env().headless);
        std::env::set_var("NEXO_PLUGIN_BROWSER_HEADLESS", "off");
        assert!(!browser_config_from_env().headless);
        clear_env();
    }

    #[test]
    #[serial]
    fn parses_user_data_dir_and_cdp_url() {
        clear_env();
        std::env::set_var("NEXO_PLUGIN_BROWSER_USER_DATA_DIR", "/tmp/profile-x");
        std::env::set_var("NEXO_PLUGIN_BROWSER_CDP_URL", "http://127.0.0.1:9222");
        let cfg = browser_config_from_env();
        assert_eq!(cfg.user_data_dir, "/tmp/profile-x");
        assert_eq!(cfg.cdp_url, "http://127.0.0.1:9222");
        clear_env();
    }
}
