//! Chrome profile decoration — writes
//! `${user_data_dir}/Default/Preferences` JSON so the Chrome
//! window's profile chip displays the agent's name + a stable
//! per-agent color.
//!
//! Mirrors the OpenClaw TypeScript reference at
//! `research/extensions/browser/src/browser/chrome.profile-decoration.ts:108-162`
//! — same two fields (`profile.name`, `profile.profile_color`),
//! same idempotent read-merge-write strategy. Pre-existing
//! Preferences keys are preserved verbatim so Chrome's other
//! settings survive across decoration calls.
//!
//! Failure modes are non-fatal: `BrowserPlugin` boot logs the
//! error via `tracing::warn!` and continues. Decoration is a
//! cosmetic operator-visibility concern, never a security
//! invariant.

use std::path::Path;

use serde_json::{Map, Value};

use crate::profile::profile_color_argb;

/// Idempotent. Reads any existing
/// `${user_data_dir}/Default/Preferences` (creating the
/// `Default/` subdir + an empty JSON object when absent),
/// inserts/updates the two `profile.*` fields we own, then
/// writes the result back atomically (write to `*.tmp` →
/// rename — same pattern Chrome itself uses for Preferences).
pub async fn decorate_profile_dir(
    user_data_dir: &Path,
    agent_id: &str,
) -> anyhow::Result<()> {
    let default_dir = user_data_dir.join("Default");
    tokio::fs::create_dir_all(&default_dir).await?;
    let prefs_path = default_dir.join("Preferences");

    // Read existing — empty / missing → start with `{}`.
    let mut root: Map<String, Value> = match tokio::fs::read(&prefs_path).await {
        Ok(bytes) if !bytes.is_empty() => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Map::new(),
    };

    // Mutate `profile.name` + `profile.profile_color`. Preserve
    // every other top-level field + every other key inside
    // `profile`.
    let profile_obj = root
        .entry("profile".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(profile_map) = profile_obj {
        profile_map.insert("name".into(), Value::String(agent_id.to_string()));
        profile_map.insert(
            "profile_color".into(),
            Value::Number(profile_color_argb(agent_id).into()),
        );
    } else {
        // Existing `profile` field was not an object — replace
        // it (Chrome would have rejected the file anyway).
        let mut profile_map = Map::new();
        profile_map.insert("name".into(), Value::String(agent_id.to_string()));
        profile_map.insert(
            "profile_color".into(),
            Value::Number(profile_color_argb(agent_id).into()),
        );
        root.insert("profile".into(), Value::Object(profile_map));
    }

    let payload = serde_json::to_vec(&Value::Object(root))?;
    let tmp_path = prefs_path.with_extension("tmp");
    tokio::fs::write(&tmp_path, &payload).await?;
    tokio::fs::rename(&tmp_path, &prefs_path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn decorate_writes_preferences_json_with_name_and_color() {
        let dir = tempdir().unwrap();
        decorate_profile_dir(dir.path(), "ana").await.unwrap();
        let prefs_path = dir.path().join("Default").join("Preferences");
        assert!(prefs_path.exists(), "Preferences file must be written");
        let bytes = tokio::fs::read(&prefs_path).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["profile"]["name"], "ana");
        let color = value["profile"]["profile_color"].as_i64().unwrap() as i32;
        assert_eq!(color, profile_color_argb("ana"));
    }

    #[tokio::test]
    async fn decorate_is_idempotent_on_repeat_call() {
        let dir = tempdir().unwrap();
        decorate_profile_dir(dir.path(), "juan").await.unwrap();
        decorate_profile_dir(dir.path(), "juan").await.unwrap();
        let prefs_path = dir.path().join("Default").join("Preferences");
        let bytes = tokio::fs::read(&prefs_path).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["profile"]["name"], "juan");
        // The file must be valid JSON after each call (no
        // accidental concatenation / corruption).
        assert!(value.get("profile").is_some());
    }

    #[tokio::test]
    async fn decorate_preserves_other_preference_fields() {
        // Pre-populate Preferences with unrelated Chrome settings
        // (the field shape Chrome itself writes). After decoration,
        // both old and new fields must coexist.
        let dir = tempdir().unwrap();
        let default_dir = dir.path().join("Default");
        tokio::fs::create_dir_all(&default_dir).await.unwrap();
        let prefs_path = default_dir.join("Preferences");
        let original = json!({
            "extensions": { "settings": { "abc": 1 } },
            "profile": {
                "exit_type": "Normal",
                "exited_cleanly": true
            }
        });
        tokio::fs::write(
            &prefs_path,
            serde_json::to_vec(&original).unwrap(),
        )
        .await
        .unwrap();

        decorate_profile_dir(dir.path(), "marketing").await.unwrap();

        let bytes = tokio::fs::read(&prefs_path).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        // Old top-level field preserved.
        assert_eq!(value["extensions"]["settings"]["abc"], 1);
        // Old `profile` keys preserved.
        assert_eq!(value["profile"]["exit_type"], "Normal");
        assert_eq!(value["profile"]["exited_cleanly"], true);
        // New `profile` keys present.
        assert_eq!(value["profile"]["name"], "marketing");
        assert!(value["profile"]["profile_color"].is_number());
    }
}
