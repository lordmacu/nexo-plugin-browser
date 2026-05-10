//! Phase 27.x.browser-windows-discovery — browser executable
//! discovery extracted into its own module so the per-platform
//! candidate lists (Linux/macOS/Windows) can grow + be tested
//! independently without bloating `chrome.rs`.
//!
//! Strategy (full design in the spec — `Tier 1 → Tier 2`):
//!
//!   1. **Tier 1 — bundled candidates.** Per-OS list of well-known
//!      install paths (`/usr/bin/google-chrome`,
//!      `C:\Program Files\Google\Chrome\Application\chrome.exe`,
//!      `/Applications/Google Chrome.app/...`). First existing path
//!      wins.
//!   2. **Tier 2 — PATH lookup.** Falls through to `which::which`
//!      so a Chrome in a custom path (corp install, Homebrew shim)
//!      that's reachable via `PATH` still resolves.
//!
//! This file ships in two waves:
//!
//! - **S3 (this commit):** the legacy Linux-only logic moves here
//!   verbatim so the rest of the workspace keeps compiling. No
//!   behavior change.
//! - **S4–S8:** swap `which_exists` for `which::which`, add Windows
//!   + macOS candidate lists + Tier 2 fallback per OS.

use std::path::Path;

/// Find the first available Chrome/Chromium executable on the
/// system. Currently Linux-only — Windows + macOS support lands
/// in S5/S6.
pub(super) fn find_chrome_executable() -> Option<String> {
    let candidates = [
        "google-chrome",
        "google-chrome-stable",
        "chromium-browser",
        "chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium-browser",
        "/usr/bin/chromium",
        "/snap/bin/chromium",
    ];

    for candidate in &candidates {
        if probe_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Cross-platform existence probe.
///
/// - Absolute paths short-circuit to a stat call so Linux's
///   `/usr/bin/...` and Windows' `C:\Program Files\...`
///   candidates resolve identically.
/// - Bare names go through `which::which` which honours the
///   platform PATH separator (`;` on Windows, `:` on POSIX) +
///   `PATHEXT` so `chrome` correctly resolves to `chrome.exe`
///   on Windows without a manual extension append.
///
/// Replaces the hand-rolled `which_exists` removed in this
/// commit, which split PATH by `:` unconditionally — broke
/// Windows (drive letters got truncated) and missed `PATHEXT`.
fn probe_exists(name: &str) -> bool {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.exists();
    }
    which::which(name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_exists_absolute_existing_returns_true() {
        // `cargo` is shipped with the toolchain so the host that
        // runs `cargo nextest` is guaranteed to have it. Resolve
        // its absolute path via the canonical lookup, then confirm
        // the absolute-path branch agrees.
        let cargo = which::which("cargo").expect("cargo on PATH");
        assert!(probe_exists(cargo.to_str().unwrap()));
    }

    #[test]
    fn probe_exists_absolute_missing_returns_false() {
        // Use a path that's deterministically absent across CI
        // images. `/nonexistent/...` is not reserved on any OS,
        // but the leading slash forces the absolute branch on
        // Unix and lets Windows test runners fall through to the
        // `Path::exists` `false` immediately too.
        assert!(!probe_exists("/nonexistent/nexo-plugin-browser-discovery-probe"));
    }

    #[test]
    fn probe_exists_bare_name_in_path() {
        // `cargo` is on PATH whenever `cargo nextest` runs the
        // test, so this exercise the `which::which` branch on
        // every platform.
        assert!(probe_exists("cargo"));
    }

    #[test]
    fn probe_exists_bare_name_not_in_path() {
        // Garbage name unlikely to clash with any binary; if a
        // future contributor names a tool this we'll know via the
        // panic. Tests `which::which` returning `Err`.
        assert!(!probe_exists("nexo-plugin-browser-this-name-must-not-exist-zzz"));
    }
}
