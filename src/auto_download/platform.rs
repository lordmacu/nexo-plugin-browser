//! Platform identifiers Google uses in the
//! [chrome-for-testing](https://googlechromelabs.github.io/chrome-for-testing/)
//! JSON catalogue.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux64,
    MacX64,
    MacArm64,
    Win32,
    Win64,
}

impl Platform {
    /// Detect the platform we're running on. Returns `None` on
    /// unsupported targets (Android, iOS, BSDs, ARM64 Windows,
    /// ARM64 Linux — Google doesn't publish a `chrome-headless-
    /// shell` build for those today).
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(Self::Linux64),
            ("macos", "x86_64") => Some(Self::MacX64),
            ("macos", "aarch64") => Some(Self::MacArm64),
            ("windows", "x86_64") => Some(Self::Win64),
            ("windows", "x86") => Some(Self::Win32),
            _ => None,
        }
    }

    /// Identifier used inside the chrome-for-testing JSON catalogue.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Linux64 => "linux64",
            Self::MacX64 => "mac-x64",
            Self::MacArm64 => "mac-arm64",
            Self::Win32 => "win32",
            Self::Win64 => "win64",
        }
    }

    /// Subdirectory name inside the downloaded zip.
    pub fn zip_root(&self) -> &'static str {
        match self {
            Self::Linux64 => "chrome-headless-shell-linux64",
            Self::MacX64 => "chrome-headless-shell-mac-x64",
            Self::MacArm64 => "chrome-headless-shell-mac-arm64",
            Self::Win32 => "chrome-headless-shell-win32",
            Self::Win64 => "chrome-headless-shell-win64",
        }
    }

    /// Binary filename Google ships inside the zip.
    pub fn binary_name(&self) -> &'static str {
        if matches!(self, Self::Win32 | Self::Win64) {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        }
    }

    /// Whether this platform needs the executable bit set after
    /// extraction. Skipped on Windows.
    pub fn needs_chmod(&self) -> bool {
        !matches!(self, Self::Win32 | Self::Win64)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_includes_exe_on_windows() {
        assert_eq!(Platform::Win64.binary_name(), "chrome-headless-shell.exe");
        assert_eq!(Platform::Linux64.binary_name(), "chrome-headless-shell");
    }

    #[test]
    fn current_resolves_on_supported_hosts() {
        let p = Platform::current();
        if cfg!(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86"),
        )) {
            assert!(p.is_some(), "expected a known platform on this host");
        }
    }
}
