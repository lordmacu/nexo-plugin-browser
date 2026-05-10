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
        if which_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// **Legacy.** Hand-rolled PATH lookup that splits by `:` —
/// breaks Windows (separator is `;`, drive letters get
/// truncated) and doesn't honour `PATHEXT`. Replaced by
/// `which::which` in S4. Kept temporarily so S3 is a pure
/// move (zero behavior change vs the removed `chrome.rs`
/// definition).
fn which_exists(name: &str) -> bool {
    // Fast check: try to find in PATH using `which` or just test if absolute path exists
    if name.starts_with('/') {
        return std::path::Path::new(name).exists();
    }
    // Search PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let full = format!("{dir}/{name}");
            if std::path::Path::new(&full).exists() {
                return true;
            }
        }
    }
    false
}
