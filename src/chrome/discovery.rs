//! Browser executable discovery: the per-platform candidate lists
//! (Linux/macOS/Windows) live here so they can grow and be tested
//! independently of `chrome.rs`.
//!
//! Strategy:
//!
//!   1. **Tier 1 — bundled candidates.** Per-OS list of well-known
//!      install paths (`/usr/bin/google-chrome`,
//!      `C:\Program Files\Google\Chrome\Application\chrome.exe`,
//!      `/Applications/Google Chrome.app/...`). First existing path
//!      wins.
//!   2. **Tier 2 — PATH lookup.** Falls through to `which::which`
//!      so a Chrome in a custom path (corp install, Homebrew shim)
//!      that's reachable via `PATH` still resolves.

use std::path::PathBuf;

use super::{BrowserExecutable, BrowserKind, DiscoveryError};


/// Pure builder for the Linux candidate list (covers Termux on
/// Android too — its prefix is a deterministic path the package
/// manager always installs to).
///
/// Unlike Windows/macOS this list is just absolute paths — no
/// per-user vs system-scope split because Linux package managers
/// install system-wide. Bare-name candidates (e.g. `google-chrome`
/// without a path) are handled by the Tier 2 PATH lookup.
#[cfg_attr(not(any(all(unix, not(target_os = "macos")), test)), allow(dead_code))]
fn linux_candidates() -> Vec<(BrowserKind, PathBuf)> {
    let pairs: &[(BrowserKind, &str)] = &[
        // Chrome: the canonical `google-chrome` name first, then
        // the `-stable` suffixed binary some distros ship.
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
/// (cfg-free builder; the runtime caller is OS-gated) so a
/// non-macOS dev box can test the candidate shape.
///
/// Each browser ships as a `.app` bundle; the actual ELF / Mach-O
/// binary lives at `<bundle>.app/Contents/MacOS/<exe-name>` per
/// the Apple bundle conventions. Both `/Applications` (admin
/// install, default for Chrome / Edge installers) and
/// `~/Applications` (rare per-user install) are checked.
///
/// Order: Chrome → Chromium → Edge.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
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
/// on the runtime caller `bundled_candidates`. Keeping the
/// builder cfg-free lets a Linux/macOS dev box validate the
/// candidate shape without spinning up a Windows runner.
///
/// Order matters: Chrome before Edge before Chromium; user
/// installs (`LOCALAPPDATA`) before system installs. First
/// existing path wins at the caller, so a corp machine where
/// Chrome lives under both `LOCALAPPDATA` and `Program Files
/// (x86)` resolves to the user-scope install (more recent in
/// practice).
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
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

/// Find the first available Chrome/Chromium/Edge executable on
/// the host. Two-tier strategy:
///
///   1. Tier 1 — bundled candidates per OS (absolute paths).
///   2. Tier 2 — PATH lookup via `which::which` (bare names).
///      Picks up Chrome installed at a non-standard path that's
///      still reachable via the operator's `PATH` (corp custom
///      install, Homebrew shim).
///
/// On `Err` the resolver lists every concrete path tested so the
/// operator can paste the list straight into a bug report or set
/// `NEXO_PLUGIN_BROWSER_EXECUTABLE` against an absolute path
/// they know exists. The `$PATH lookup: <name>` rows make Tier 2
/// failures self-explanatory.
pub(super) fn find_chrome_executable() -> Result<BrowserExecutable, DiscoveryError> {
    let mut searched: Vec<String> = Vec::new();

    // Tier 1 — bundled per-OS candidate list.
    for (kind, path) in bundled_candidates() {
        let path_str = path.to_string_lossy().into_owned();
        if path.exists() {
            return Ok(BrowserExecutable { kind, path });
        }
        searched.push(path_str);
    }

    // Tier 2 — PATH lookup.
    for (kind, name) in path_lookup_names() {
        // Annotate the entry so the operator can tell a bundled
        // miss (`/usr/bin/google-chrome`) from a PATH miss
        // (`$PATH lookup: chrome`).
        searched.push(format!("$PATH lookup: {name}"));
        if let Ok(path) = which::which(name) {
            return Ok(BrowserExecutable {
                kind: kind.clone(),
                path,
            });
        }
    }

    Err(DiscoveryError::NotFound { searched })
}

/// Bundled candidate list for the current host — dispatched per
/// OS. Returns absolute paths; the resolver probes each with
/// `Path::exists`.
fn bundled_candidates() -> Vec<(BrowserKind, PathBuf)> {
    #[cfg(target_os = "windows")]
    {
        let local_app = std::env::var("LOCALAPPDATA").ok();
        let program_files = std::env::var("ProgramFiles")
            .unwrap_or_else(|_| String::from(r"C:\Program Files"));
        let program_files_x86 = std::env::var("ProgramFiles(x86)")
            .unwrap_or_else(|_| String::from(r"C:\Program Files (x86)"));
        return windows_candidates(local_app.as_deref(), &program_files, &program_files_x86);
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok();
        return macos_candidates(home.as_deref());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return linux_candidates();
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        Vec::new()
    }
}

/// Tier 2 — PATH lookup name list for the current host.
fn path_lookup_names() -> &'static [(BrowserKind, &'static str)] {
    #[cfg(target_os = "windows")]
    {
        // `chrome` resolves to `chrome.exe` via PATHEXT.
        return &[
            (BrowserKind::Chrome, "chrome"),
            (BrowserKind::Edge, "msedge"),
        ];
    }
    #[cfg(target_os = "macos")]
    {
        // Homebrew Cask sometimes drops these as shims.
        return &[
            (BrowserKind::Chrome, "google-chrome"),
            (BrowserKind::Chromium, "chromium"),
        ];
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Bare names so a custom apt repo dropping Chrome
        // outside `/usr/bin` still resolves via PATH.
        return &[
            (BrowserKind::Chrome, "google-chrome"),
            (BrowserKind::Chrome, "google-chrome-stable"),
            (BrowserKind::Chromium, "chromium-browser"),
            (BrowserKind::Chromium, "chromium"),
        ];
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        &[]
    }
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_cargo_on_test_host() {
        // `cargo` is shipped with the toolchain so the host that
        // runs `cargo nextest` is guaranteed to have it. Smoke
        // check that the `which::which` we lean on for Tier 2
        // resolves a known-present binary on every platform.
        assert!(which::which("cargo").is_ok(), "cargo must be on test host PATH");
    }

    #[test]
    fn which_returns_err_for_garbage_name() {
        // Sentinel name unlikely to clash with any real binary.
        // Defends against a future `which` upgrade returning Ok
        // for a non-existent name.
        assert!(which::which("nexo-plugin-browser-this-must-not-exist-zzz").is_err());
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
        assert!(paths.iter().any(|p| p == "/usr/bin/google-chrome"));
        assert!(paths.iter().any(|p| p == "/usr/bin/google-chrome-stable"));
        assert!(paths.iter().any(|p| p == "/usr/bin/chromium-browser"));
        assert!(paths.iter().any(|p| p == "/usr/bin/chromium"));
        assert!(paths.iter().any(|p| p == "/snap/bin/chromium"));
        // Termux Android prefix.
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
        // Match the trailing `Applications/...` suffix + a `Users/foo`
        // marker rather than the whole literal: `PathBuf::join` glues the
        // home prefix on with the host separator, so on a Windows test
        // host the join boundary is a backslash even though every other
        // segment (and the runtime macOS path) stays `/`.
        assert!(
            paths.iter().any(|p| p.ends_with("Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
                && p.contains("Users/foo")),
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
    fn bundled_candidates_dispatches_per_host() {
        // Smoke check: the host-dispatcher returns the expected
        // per-OS shape. Useful regression guard if a future
        // contributor adds a new platform branch and forgets to
        // wire it through.
        let cands = bundled_candidates();
        assert!(!cands.is_empty(), "host should produce at least one candidate");
    }

    #[test]
    fn path_lookup_names_dispatches_per_host() {
        let names = path_lookup_names();
        assert!(!names.is_empty(), "host should have at least one PATH lookup name");
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
