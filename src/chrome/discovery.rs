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

/// macOS bundled candidates — `.app` bundles in `/Applications`
/// (admin install) and `~/Applications` (per-user install,
/// rare but legitimate). The pure builder
/// `macos_candidates(home)` is cfg-free for testability.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // wired up in S8
fn find_in_candidates() -> Option<BrowserExecutable> {
    let home = std::env::var("HOME").ok();
    let candidates = macos_candidates(home.as_deref());

    candidates
        .into_iter()
        .find(|(_, p)| p.exists())
        .map(|(kind, path)| BrowserExecutable { kind, path })
}

/// Linux bundled candidates — `apt`/`snap` install paths plus
/// the Termux Android prefix. Same shape as `windows_candidates`
/// / `macos_candidates`: pure builder for testability, runtime
/// wrapper handles the disk probe.
#[cfg(all(unix, not(target_os = "macos")))]
#[allow(dead_code)] // wired up in S8
fn find_in_candidates() -> Option<BrowserExecutable> {
    let candidates = linux_candidates();
    candidates
        .into_iter()
        .find(|(_, p)| p.exists())
        .map(|(kind, path)| BrowserExecutable { kind, path })
}

/// Pure builder for the Linux candidate list (covers Termux on
/// Android too — its prefix is a deterministic path the package
/// manager always installs to).
///
/// Unlike Windows/macOS this list is just absolute paths — no
/// per-user vs system-scope split because Linux package managers
/// install system-wide. The bare-name candidates that the legacy
/// `find_chrome_executable` used (e.g. `google-chrome` without a
/// path) move into the Tier 2 `find_in_path` PATH lookup added
/// in S8.
fn linux_candidates() -> Vec<(BrowserKind, PathBuf)> {
    let pairs: &[(BrowserKind, &str)] = &[
        // Chrome stable + the legacy non-suffixed binary that
        // some distros symlink. Order: stable → unsuffixed
        // (matches the legacy list's preference for the
        // canonical `google-chrome` name).
        (BrowserKind::Chrome, "/usr/bin/google-chrome"),
        (BrowserKind::Chrome, "/usr/bin/google-chrome-stable"),
        // Chromium variants — `chromium-browser` is what Debian /
        // Ubuntu ship; `chromium` is the upstream Chromium
        // project name + Snap target name.
        (BrowserKind::Chromium, "/usr/bin/chromium-browser"),
        (BrowserKind::Chromium, "/usr/bin/chromium"),
        (BrowserKind::Chromium, "/snap/bin/chromium"),
        // Termux Android — `pkg install chromium` installs to a
        // deterministic prefix under the Termux app sandbox.
        // Including the path here means a bare Termux operator
        // running this plugin without `NEXO_PLUGIN_BROWSER_EXECUTABLE`
        // gets auto-detect for free, matching the Linux UX.
        (BrowserKind::Chromium, "/data/data/com.termux/files/usr/bin/chromium"),
    ];

    pairs.iter().map(|(k, p)| (k.clone(), PathBuf::from(*p))).collect()
}

/// Pure builder for the macOS candidate list. Always compiled
/// — see `windows_candidates` rationale.
///
/// Each browser ships as a `.app` bundle; the actual ELF / Mach-O
/// binary lives at `<bundle>.app/Contents/MacOS/<exe-name>` per
/// the Apple bundle conventions. Both `/Applications` (admin
/// install, default for Chrome / Edge installers) and
/// `~/Applications` (rare per-user install) are checked.
///
/// Order: Chrome → Chromium → Edge.
fn macos_candidates(home: Option<&str>) -> Vec<(BrowserKind, PathBuf)> {
    // System-scope `/Applications/...` paths first so
    // operators with a global Chrome install hit it before any
    // accidental personal copy under `~/Applications`.
    let system_pairs: &[(BrowserKind, &str)] = &[
        (
            BrowserKind::Chrome,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (
            BrowserKind::Chromium,
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ),
        (
            BrowserKind::Edge,
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ),
    ];

    let mut out: Vec<(BrowserKind, PathBuf)> = Vec::with_capacity(6);
    for (kind, path) in system_pairs {
        out.push((kind.clone(), PathBuf::from(*path)));
    }

    if let Some(h) = home {
        // Strip the leading `/` on the system path so `Path::join`
        // attaches it relative to `<home>` rather than treating
        // it as an absolute that resets the join target.
        for (kind, path) in system_pairs {
            let suffix = path.trim_start_matches('/');
            out.push((kind.clone(), PathBuf::from(h).join(suffix)));
        }
    }

    out
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
    fn linux_candidates_include_chrome_chromium_termux() {
        let cands = linux_candidates();
        let paths: Vec<_> = cands.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        // Pre-existing path coverage preserved (regression guard).
        assert!(paths.iter().any(|p| p == "/usr/bin/google-chrome"));
        assert!(paths.iter().any(|p| p == "/usr/bin/google-chrome-stable"));
        assert!(paths.iter().any(|p| p == "/usr/bin/chromium-browser"));
        assert!(paths.iter().any(|p| p == "/usr/bin/chromium"));
        assert!(paths.iter().any(|p| p == "/snap/bin/chromium"));
        // New: Termux Android prefix.
        assert!(
            paths.iter().any(|p| p == "/data/data/com.termux/files/usr/bin/chromium"),
            "Termux Android path missing: {paths:?}"
        );
    }

    #[test]
    fn linux_candidates_chrome_ranks_before_chromium() {
        let cands = linux_candidates();
        let chrome_idx = cands
            .iter()
            .position(|(k, _)| matches!(k, BrowserKind::Chrome))
            .expect("at least one Chrome entry");
        let chromium_idx = cands
            .iter()
            .position(|(k, _)| matches!(k, BrowserKind::Chromium))
            .expect("at least one Chromium entry");
        assert!(chrome_idx < chromium_idx, "Chrome should rank before Chromium");
    }

    #[test]
    fn macos_candidates_with_home_includes_user_paths() {
        let cands = macos_candidates(Some("/Users/foo"));
        // 3 system + 3 user = 6.
        assert_eq!(cands.len(), 6);
        let paths: Vec<_> = cands.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        assert!(
            paths.iter().any(|p| p == "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "system Chrome missing: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p
                == "/Users/foo/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "user Chrome missing: {paths:?}"
        );
    }

    #[test]
    fn macos_candidates_without_home_falls_back_to_system() {
        let cands = macos_candidates(None);
        // Only 3 system entries.
        assert_eq!(cands.len(), 3);
    }

    #[test]
    fn macos_candidates_system_ranks_before_user() {
        // Operator preference: a global install wins over a
        // personal copy. Anchors the Vec ordering against drift.
        let cands = macos_candidates(Some("/Users/foo"));
        let first_user_idx = cands
            .iter()
            .position(|(_, p)| p.to_string_lossy().contains("/Users/foo"))
            .expect("at least one user-scope entry");
        // All entries before the first user-scope must be system-scope.
        for (idx, (_, p)) in cands.iter().enumerate() {
            if idx >= first_user_idx { break; }
            assert!(
                p.starts_with("/Applications"),
                "expected /Applications prefix at idx {idx}, got {}",
                p.display()
            );
        }
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
