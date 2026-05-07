//! Env-controlled limits for the per-agent profile DashMap.
//!
//! Read once at subprocess boot from
//! `read_profile_limits()`; mid-run env changes are ignored
//! (matches POSIX semantics — a daemon restart picks up the
//! new value). Out-of-range values are clamped + warn-logged
//! so a YAML typo doesn't crash boot.
//!
//! Three knobs:
//!
//!   * `NEXO_PLUGIN_BROWSER_MAX_PROFILES` — cap on simultaneous
//!     active Chrome profiles. Range `[1, 64]`. Default `10`.
//!   * `NEXO_PLUGIN_BROWSER_PROFILE_IDLE_SECS` — idle threshold
//!     for the eviction loop. Range `[0, 86400]`. `0` disables
//!     eviction. Default `900` (15 min).
//!   * `NEXO_PLUGIN_BROWSER_MULTI_PROFILE` — `true` / `false`.
//!     When `false`, all agents share the legacy single-profile
//!     `user_data_dir` (v0.2.0 backwards-compat). Default
//!     `true`.

const ENV_MAX_PROFILES: &str = "NEXO_PLUGIN_BROWSER_MAX_PROFILES";
const ENV_IDLE_SECS: &str = "NEXO_PLUGIN_BROWSER_PROFILE_IDLE_SECS";
const ENV_MULTI_PROFILE: &str = "NEXO_PLUGIN_BROWSER_MULTI_PROFILE";

const DEFAULT_MAX_PROFILES: usize = 10;
const MIN_MAX_PROFILES: usize = 1;
const MAX_MAX_PROFILES: usize = 64;

const DEFAULT_IDLE_SECS: u64 = 900;
const MIN_IDLE_SECS: u64 = 0;
const MAX_IDLE_SECS: u64 = 86_400;

/// Resolved env knobs. Cheap to clone (3 small fields) so the
/// caller can pass it by value into per-call resolvers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileLimits {
    /// Cap on simultaneous active profiles. Always in
    /// `[MIN_MAX_PROFILES, MAX_MAX_PROFILES]`.
    pub max_profiles: usize,
    /// Idle threshold in seconds. `0` disables eviction;
    /// otherwise in `[1, MAX_IDLE_SECS]`.
    pub idle_secs: u64,
    /// `false` = legacy single-shared-profile mode (all agents
    /// share `user_data_dir`).
    pub multi_profile_enabled: bool,
}

impl Default for ProfileLimits {
    fn default() -> Self {
        Self {
            max_profiles: DEFAULT_MAX_PROFILES,
            idle_secs: DEFAULT_IDLE_SECS,
            multi_profile_enabled: true,
        }
    }
}

/// Read all three env knobs once. Out-of-range values are
/// clamped + warn-logged. Unparseable values fall back to the
/// default + warn.
pub fn read_profile_limits() -> ProfileLimits {
    ProfileLimits {
        max_profiles: read_max_profiles(),
        idle_secs: read_idle_secs(),
        multi_profile_enabled: read_multi_profile(),
    }
}

fn read_max_profiles() -> usize {
    let Some(raw) = std::env::var(ENV_MAX_PROFILES).ok() else {
        return DEFAULT_MAX_PROFILES;
    };
    let parsed: usize = match raw.trim().parse() {
        Ok(n) => n,
        Err(_) => {
            tracing::warn!(
                env = ENV_MAX_PROFILES,
                value = %raw,
                default = DEFAULT_MAX_PROFILES,
                "unparseable; using default"
            );
            return DEFAULT_MAX_PROFILES;
        }
    };
    if parsed < MIN_MAX_PROFILES {
        tracing::warn!(
            env = ENV_MAX_PROFILES,
            value = parsed,
            min = MIN_MAX_PROFILES,
            "below minimum; clamping"
        );
        return MIN_MAX_PROFILES;
    }
    if parsed > MAX_MAX_PROFILES {
        tracing::warn!(
            env = ENV_MAX_PROFILES,
            value = parsed,
            max = MAX_MAX_PROFILES,
            "above maximum; clamping"
        );
        return MAX_MAX_PROFILES;
    }
    parsed
}

fn read_idle_secs() -> u64 {
    let Some(raw) = std::env::var(ENV_IDLE_SECS).ok() else {
        return DEFAULT_IDLE_SECS;
    };
    let parsed: u64 = match raw.trim().parse() {
        Ok(n) => n,
        Err(_) => {
            tracing::warn!(
                env = ENV_IDLE_SECS,
                value = %raw,
                default = DEFAULT_IDLE_SECS,
                "unparseable; using default"
            );
            return DEFAULT_IDLE_SECS;
        }
    };
    if parsed < MIN_IDLE_SECS {
        // u64 can't be negative — branch unreachable; kept for
        // symmetry / future-proofing if MIN_IDLE_SECS rises.
        return MIN_IDLE_SECS;
    }
    if parsed > MAX_IDLE_SECS {
        tracing::warn!(
            env = ENV_IDLE_SECS,
            value = parsed,
            max = MAX_IDLE_SECS,
            "above maximum; clamping"
        );
        return MAX_IDLE_SECS;
    }
    parsed
}

fn read_multi_profile() -> bool {
    let Some(raw) = std::env::var(ENV_MULTI_PROFILE).ok() else {
        return true;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => {
            tracing::warn!(
                env = ENV_MULTI_PROFILE,
                value = %raw,
                "unparseable; defaulting to true"
            );
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn clear_env() {
        for k in [ENV_MAX_PROFILES, ENV_IDLE_SECS, ENV_MULTI_PROFILE] {
            std::env::remove_var(k);
        }
    }

    #[test]
    #[serial]
    fn defaults_when_no_env() {
        clear_env();
        let limits = read_profile_limits();
        assert_eq!(limits.max_profiles, DEFAULT_MAX_PROFILES);
        assert_eq!(limits.idle_secs, DEFAULT_IDLE_SECS);
        assert!(limits.multi_profile_enabled);
    }

    #[test]
    #[serial]
    fn parses_valid_values() {
        clear_env();
        std::env::set_var(ENV_MAX_PROFILES, "5");
        std::env::set_var(ENV_IDLE_SECS, "60");
        std::env::set_var(ENV_MULTI_PROFILE, "false");
        let limits = read_profile_limits();
        assert_eq!(limits.max_profiles, 5);
        assert_eq!(limits.idle_secs, 60);
        assert!(!limits.multi_profile_enabled);
        clear_env();
    }

    #[test]
    #[serial]
    fn clamps_max_profiles_below_minimum() {
        clear_env();
        std::env::set_var(ENV_MAX_PROFILES, "0");
        let limits = read_profile_limits();
        assert_eq!(limits.max_profiles, MIN_MAX_PROFILES);
        clear_env();
    }

    #[test]
    #[serial]
    fn clamps_max_profiles_above_maximum() {
        clear_env();
        std::env::set_var(ENV_MAX_PROFILES, "10000");
        let limits = read_profile_limits();
        assert_eq!(limits.max_profiles, MAX_MAX_PROFILES);
        clear_env();
    }

    #[test]
    #[serial]
    fn clamps_idle_secs_above_max() {
        clear_env();
        std::env::set_var(ENV_IDLE_SECS, "999999");
        let limits = read_profile_limits();
        assert_eq!(limits.idle_secs, MAX_IDLE_SECS);
        clear_env();
    }

    #[test]
    #[serial]
    fn idle_zero_disables_eviction() {
        clear_env();
        std::env::set_var(ENV_IDLE_SECS, "0");
        let limits = read_profile_limits();
        assert_eq!(limits.idle_secs, 0);
        clear_env();
    }

    #[test]
    #[serial]
    fn multi_profile_accepts_truthy_and_falsy_strings() {
        clear_env();
        std::env::set_var(ENV_MULTI_PROFILE, "yes");
        assert!(read_profile_limits().multi_profile_enabled);
        std::env::set_var(ENV_MULTI_PROFILE, "off");
        assert!(!read_profile_limits().multi_profile_enabled);
        clear_env();
    }

    #[test]
    #[serial]
    fn unparseable_max_profiles_falls_back_to_default() {
        clear_env();
        std::env::set_var(ENV_MAX_PROFILES, "abc");
        let limits = read_profile_limits();
        assert_eq!(limits.max_profiles, DEFAULT_MAX_PROFILES);
        clear_env();
    }
}
