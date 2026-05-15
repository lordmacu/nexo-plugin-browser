//! Filesystem-safe agent_id sanitiser + per-agent profile dir
//! resolver + deterministic ARGB color derivation for Chrome
//! profile decoration.
//!
//! The standalone subprocess holds a
//! `DashMap<String, ProfileEntry>` keyed on the sanitised
//! `agent_id` from each [`tool.invoke`] request. This module
//! provides the three pure helpers `main.rs` calls before
//! lazy-booting Chrome:
//!
//!   * [`sanitize_agent_id`] — convert the wire-format
//!     `agent_id` into a filesystem-safe path component.
//!   * [`user_data_dir_for`] — `${BASE}/profiles/<agent_id>/`.
//!   * [`profile_color_argb`] — stable ARGB int per agent for
//!     the Chrome profile chip.
//!
//! The sanitiser is intentionally strict: it accepts only the
//! same `[A-Za-z0-9_-]{1,64}` regex `nexo-plugin-manifest::id_regex`
//! enforces for plugin / agent ids, so operators have one mental
//! model across the framework.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_AGENT_ID_LEN: usize = 64;

/// Errors returned by [`sanitize_agent_id`]. Wrapped through the
/// caller into `ToolInvocationError::ArgumentInvalid` (see
/// `src/main.rs::shared_plugin_for`).
/// Canonical alias for [`ProfileIdError`]. Step 2 of the
/// browser-multi-instance migration introduces the
/// instance-namespace-aware name; both refer to the same enum so
/// existing 0.2.x callers compile unchanged.
pub type IdError = ProfileIdError;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProfileIdError {
    /// `agent_id` was empty or whitespace-only after trimming.
    #[error("agent_id is empty")]
    Empty,
    /// `agent_id` exceeds the [`MAX_AGENT_ID_LEN`] character cap.
    #[error("agent_id `{0}` exceeds 64 characters")]
    TooLong(String),
    /// `agent_id` contains a character outside the allowed set.
    /// Path traversal (`..`, `/`), control bytes, and Unicode
    /// punctuation are all rejected here.
    #[error("agent_id `{0}` contains invalid characters (allowed: [A-Za-z0-9_-])")]
    Invalid(String),
}

/// Lowercase + validate the wire-format `agent_id`. Returns the
/// owned canonical key ready for use as a `DashMap` lookup AND
/// as a filesystem path component.
///
/// Rules (regex `^[a-z0-9_-]{1,64}$` after lowercasing):
///   * empty / whitespace-only → [`ProfileIdError::Empty`].
///   * length > 64 → [`ProfileIdError::TooLong`].
///   * any char outside `[A-Za-z0-9_-]` → [`ProfileIdError::Invalid`].
pub fn sanitize_id(raw: &str) -> Result<String, IdError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ProfileIdError::Empty);
    }
    if trimmed.chars().count() > MAX_AGENT_ID_LEN {
        return Err(ProfileIdError::TooLong(trimmed.to_string()));
    }
    for c in trimmed.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(ProfileIdError::Invalid(trimmed.to_string()));
        }
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Deprecated alias of [`sanitize_id`]. Kept for 0.2.x callers.
#[deprecated(since = "0.3.0", note = "use sanitize_id (namespace-neutral name)")]
pub fn sanitize_agent_id(raw: &str) -> Result<String, ProfileIdError> {
    sanitize_id(raw)
}

/// Compose the per-agent Chrome `user_data_dir`:
/// `${base}/profiles/<agent_id>/`.
///
/// Pure path-join — no filesystem IO. The caller (Chrome
/// launcher) creates the directory lazily during the boot
/// sequence; subsequent boots reuse the existing tree (which is
/// how Chrome persists cookies / localStorage between runs).
///
/// `agent_id` MUST already be sanitised via [`sanitize_agent_id`];
/// this function is a thin join, not a validator. Debug builds
/// assert the invariant.
pub fn user_data_dir_for(base: &Path, agent_id: &str) -> PathBuf {
    debug_assert!(
        sanitize_id(agent_id).is_ok(),
        "user_data_dir_for called with un-sanitised agent_id `{agent_id}`"
    );
    base.join("profiles").join(agent_id)
}

/// Stable ARGB int derived from `sha256(agent_id)`'s first 3
/// bytes (used as RGB) plus a forced `0xFF` alpha. Used by
/// [`crate::profile_decoration::decorate_profile_dir`] to
/// stamp the Chrome profile chip with a per-agent color so
/// operators eyeballing N parallel Chromes can tell them apart
/// at a glance.
pub fn profile_color_argb(agent_id: &str) -> i32 {
    let digest = Sha256::digest(agent_id.as_bytes());
    let r = digest[0] as u32;
    let g = digest[1] as u32;
    let b = digest[2] as u32;
    let argb_u32 = (0xFFu32 << 24) | (r << 16) | (g << 8) | b;
    argb_u32 as i32
}

#[cfg(test)]
#[allow(deprecated)] // existing tests call the 0.2.x alias; 1 new test below targets `sanitize_id`.
mod tests {
    use super::*;

    #[test]
    fn sanitize_id_is_canonical_entry_point() {
        // Canonical 0.3.0 name. Behaviour-identical to the
        // deprecated `sanitize_agent_id` alias; this test pins
        // the public name so callers can migrate.
        assert_eq!(sanitize_id("Ana").unwrap(), "ana");
        assert_eq!(sanitize_id("").unwrap_err(), ProfileIdError::Empty);
    }

    // ── sanitize_agent_id ────────────────────────────────────

    #[test]
    fn sanitize_lowercases_and_returns_owned() {
        assert_eq!(sanitize_agent_id("Ana").unwrap(), "ana");
    }

    #[test]
    fn sanitize_accepts_underscores_and_hyphens() {
        assert_eq!(sanitize_agent_id("agent_x-1").unwrap(), "agent_x-1");
    }

    #[test]
    fn sanitize_accepts_max_length_64_chars() {
        let s = "a".repeat(64);
        assert_eq!(sanitize_agent_id(&s).unwrap(), s);
    }

    #[test]
    fn sanitize_trims_surrounding_whitespace() {
        assert_eq!(sanitize_agent_id("  ana  ").unwrap(), "ana");
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert_eq!(sanitize_agent_id("").unwrap_err(), ProfileIdError::Empty);
    }

    #[test]
    fn sanitize_rejects_whitespace_only() {
        assert_eq!(
            sanitize_agent_id("   ").unwrap_err(),
            ProfileIdError::Empty
        );
    }

    #[test]
    fn sanitize_rejects_too_long_65_chars() {
        let s = "a".repeat(65);
        match sanitize_agent_id(&s).unwrap_err() {
            ProfileIdError::TooLong(got) => assert_eq!(got, s),
            other => panic!("expected TooLong, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_rejects_path_traversal_dot_dot() {
        match sanitize_agent_id("..").unwrap_err() {
            ProfileIdError::Invalid(got) => assert_eq!(got, ".."),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_rejects_forward_slash() {
        match sanitize_agent_id("a/b").unwrap_err() {
            ProfileIdError::Invalid(got) => assert_eq!(got, "a/b"),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_rejects_control_chars() {
        match sanitize_agent_id("\x01ana").unwrap_err() {
            ProfileIdError::Invalid(_) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_rejects_unicode_punctuation() {
        match sanitize_agent_id("ana.es").unwrap_err() {
            ProfileIdError::Invalid(got) => assert_eq!(got, "ana.es"),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_rejects_non_ascii_letters() {
        // Sanitiser is intentionally ASCII-only — Chrome profile
        // dirs should be portable across locales / filesystem
        // encodings. Operators with non-ASCII agent ids must
        // pick an ASCII alias.
        match sanitize_agent_id("ana-ñ").unwrap_err() {
            ProfileIdError::Invalid(_) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    // ── user_data_dir_for ───────────────────────────────────

    #[test]
    fn user_data_dir_for_joins_profiles_subdir() {
        let dir = user_data_dir_for(Path::new("/tmp/state"), "ana");
        assert_eq!(dir, PathBuf::from("/tmp/state/profiles/ana"));
    }

    #[test]
    fn user_data_dir_for_handles_relative_base() {
        let dir = user_data_dir_for(Path::new("./.profile"), "x");
        assert_eq!(dir, PathBuf::from("./.profile/profiles/x"));
    }

    // ── profile_color_argb ──────────────────────────────────

    #[test]
    fn profile_color_argb_deterministic_across_calls() {
        assert_eq!(profile_color_argb("ana"), profile_color_argb("ana"));
        assert_eq!(profile_color_argb("juan"), profile_color_argb("juan"));
    }

    #[test]
    fn profile_color_argb_alpha_byte_is_ff() {
        let argb = profile_color_argb("ana") as u32;
        let alpha = (argb >> 24) & 0xFF;
        assert_eq!(alpha, 0xFF, "alpha must be 0xFF, got 0x{:02x}", alpha);
    }

    #[test]
    fn profile_color_argb_distinct_for_distinct_agents_probabilistic() {
        // 3-byte prefix collisions are 1-in-2^24; for the small
        // sample below we expect distinct outputs.
        let agents = ["ana", "juan", "marketing", "tester", "default"];
        let mut seen = std::collections::HashSet::new();
        for agent in agents {
            let argb = profile_color_argb(agent);
            assert!(
                seen.insert(argb),
                "color collision for `{agent}` (0x{:08x}) within sample",
                argb as u32
            );
        }
    }
}
