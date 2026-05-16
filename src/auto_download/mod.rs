//! Tier 0 of the discovery flow — auto-download Google's
//! `chrome-headless-shell` instead of requiring a system-
//! installed Chrome.
//!
//! Vendored from the prototype `chrome-for-testing` crate so
//! the plugin can publish to crates.io with no external git /
//! path deps. The footprint is small: one platform enum, one
//! manager that talks JSON + reqwest + zip + tokio.
//!
//! Gated entirely on the `auto-download` cargo feature — when
//! the feature is off this module isn't compiled at all.

pub mod manager;
pub mod platform;

pub use manager::{ensure_chrome_headless_shell, Manager};
pub use platform::Platform;
