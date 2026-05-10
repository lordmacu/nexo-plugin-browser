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

use std::path::{Path, PathBuf};

use super::{BrowserExecutable, BrowserKind};

/// Tier 1 — bundled candidate paths per OS. Returns the first
/// candidate that exists on disk, or `None` if every well-known
/// install path is empty (caller falls through to the Tier 2
/// PATH lookup added in S8).
///
/// Each per-OS branch defers candidate construction to a pure
/// `*_candidates(env_vars...)` builder so the lists can be tested
/// without filesystem access on any host (a Linux dev box can
/// validate the Windows candidate shape).
#[cfg(target_os = "windows")]
#[allow(dead_code)] // wired up to `find_chrome_executable` in S8
fn find_in_candidates() -> Option<BrowserExecutable> {
    let local_app = std::env::var("LOCALAPPDATA").ok();
    let program_files =
        std::env::var("ProgramFiles").unwrap_or_else(|_| String::from(r"C:\Program Files"));
    // `std::env::var` accepts the parenthesized name verbatim;
    // call it out so the next reader doesn't reach for shell
    // expansion or a `${...}` form.
    let program_files_x86 = std::env::var("ProgramFiles(x86)")
        .unwrap_or_else(|_| String::from(r"C:\Program Files (x86)"));

    let candidates = windows_candidates(local_app.as_deref(), &program_files, &program_files_x86);

    candidates
        .into_iter()
        .find(|(_, p)| p.exists())
        .map(|(kind, path)| BrowserExecutable { kind, path })
}

/// macOS bundled candidates land in S6.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // wired up in S8
fn find_in_candidates() -> Option<BrowserExecutable> {
    None
}

/// Linux bundled candidates land in S7 — until then this
/// function returns `None` and the legacy
/// `find_chrome_executable` (still used by `chrome.rs::launch`)
/// keeps Linux running through its own list.
#[cfg(all(unix, not(target_os = "macos")))]
#[allow(dead_code)] // wired up in S8
fn find_in_candidates() -> Option<BrowserExecutable> {
    None
}

/// Pure builder for the Windows candidate list. Always
/// compiled — the `#[cfg(target_os = "windows")]` guard lives
/// on the runtime caller `find_in_candidates`. Keeping the
/// builder cfg-free lets a Linux/macOS dev box validate the
/// candidate shape without spinning up a Windows runner.
///
/// Order matters: Chrome before Edge before Chromium; user
/// installs (`LOCALAPPDATA`) before system installs. First
/// existing path wins at the caller, so a corp machine where
/// Chrome lives under both `LOCALAPPDATA` and `Program Files
/// (x86)` resolves to the user-scope install (more recent in
/// practice).
fn windows_candidates(
    local_app: Option<&str>,
    program_files: &str,
    program_files_x86: &str,
) -> Vec<(BrowserKind, PathBuf)> {
    let mut out: Vec<(BrowserKind, PathBuf)> = Vec::with_capacity(7);

    if let Some(la) = local_app {
        out.push((
            BrowserKind::Chrome,
            PathBuf::from(la).join(r"Google\Chrome\Application\chrome.exe"),
        ));
        out.push((
            BrowserKind::Edge,
            PathBuf::from(la).join(r"Microsoft\Edge\Application\msedge.exe"),
        ));
        out.push((
            BrowserKind::Chromium,
            PathBuf::from(la).join(r"Chromium\Application\chrome.exe"),
        ));
    }

    out.push((
        BrowserKind::Chrome,
        PathBuf::from(program_files).join(r"Google\Chrome\Application\chrome.exe"),
    ));
    out.push((
        BrowserKind::Chrome,
        PathBuf::from(program_files_x86).join(r"Google\Chrome\Application\chrome.exe"),
    ));
    out.push((
        BrowserKind::Edge,
        PathBuf::from(program_files).join(r"Microsoft\Edge\Application\msedge.exe"),
    ));
    out.push((
        BrowserKind::Edge,
        PathBuf::from(program_files_x86).join(r"Microsoft\Edge\Application\msedge.exe"),
    ));

    out
}

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

    #[test]
    fn windows_candidates_with_localappdata_includes_user_paths() {
        let cands = windows_candidates(
            Some(r"C:\Users\foo\AppData\Local"),
            r"C:\Program Files",
            r"C:\Program Files (x86)",
        );
        // 3 user-scope (Chrome, Edge, Chromium) + 4 system-scope
        // (Chrome × 2, Edge × 2) = 7 entries.
        assert_eq!(cands.len(), 7);

        // Assert on file-name + parent suffix only — `PathBuf::join`
        // uses the host separator, so on Linux the prefix and the
        // joined component end up split by `/` even though the
        // literals are `\`-separated. Checking the trailing
        // join-boundary backslash sequence is portable.
        let paths: Vec<_> = cands.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with(r"Google\Chrome\Application\chrome.exe")
                && p.contains("AppData")),
            "user-scope Chrome missing: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.ends_with(r"Microsoft\Edge\Application\msedge.exe")
                && p.contains("AppData")),
            "user-scope Edge missing: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.ends_with(r"Chromium\Application\chrome.exe")
                && p.contains("AppData")),
            "user-scope Chromium missing: {paths:?}"
        );
    }

    #[test]
    fn windows_candidates_without_localappdata_falls_back_to_system() {
        let cands = windows_candidates(None, r"C:\Program Files", r"C:\Program Files (x86)");
        // Just the 4 system-scope entries.
        assert_eq!(cands.len(), 4);
        let paths: Vec<_> = cands.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        assert!(paths.iter().all(|p| !p.contains("AppData")), "no user paths expected: {paths:?}");
    }

    #[test]
    fn windows_candidates_include_program_files_x86() {
        // Defends against accidentally dropping the 32-bit-on-64
        // variant, which is what a `Program Files (x86)` install
        // of Chrome looks like on a 64-bit Windows host. Match on
        // sentinel substrings — `PathBuf::join` uses the host
        // separator, so `starts_with` against a backslash-only
        // prefix breaks on Linux even though the runtime path on
        // Windows resolves correctly.
        let cands = windows_candidates(None, r"C:\PF", r"C:\PFx86");
        let paths: Vec<_> = cands.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        assert!(
            paths.iter().any(|p| p.contains("PFx86") && p.ends_with(r"Google\Chrome\Application\chrome.exe")),
            "Program Files (x86) Chrome missing: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.contains("PFx86") && p.ends_with(r"Microsoft\Edge\Application\msedge.exe")),
            "Program Files (x86) Edge missing: {paths:?}"
        );
    }

    #[test]
    fn windows_candidates_chrome_ranks_before_edge() {
        // Operator preference: when both Chrome and Edge are
        // installed in the same scope, Chrome wins. Asserting on
        // Vec order — the first existing path wins at the caller.
        let cands = windows_candidates(Some(r"C:\Local"), r"C:\PF", r"C:\PFx86");
        let chrome_idx = cands
            .iter()
            .position(|(k, _)| matches!(k, BrowserKind::Chrome))
            .expect("at least one Chrome entry");
        let edge_idx = cands
            .iter()
            .position(|(k, _)| matches!(k, BrowserKind::Edge))
            .expect("at least one Edge entry");
        assert!(chrome_idx < edge_idx, "Chrome should rank before Edge");
    }
}
