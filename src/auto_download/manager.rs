//! `chrome-headless-shell` lifecycle: discover Google's JSON
//! catalogue, download the zip for the running platform, extract
//! it under `$XDG_CACHE_HOME/nexo-plugin-browser/chrome-for-testing/<version>/`,
//! `chmod +x` on Unix, return the absolute binary path.
//!
//! This module is the (vendored) substance of what was previously
//! a separate `chrome-for-testing` crate. We embed it directly
//! so the plugin can publish to crates.io without a transitive
//! dep that isn't there.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use super::platform::Platform;

/// Google's catalogue of last-known-good Chrome for Testing builds.
const VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// Detect platform, ensure binary cached, return absolute path.
/// Idempotent — re-running on a cache hit is near-free.
pub async fn ensure_chrome_headless_shell() -> Result<PathBuf> {
    Manager::new()?.ensure().await
}

pub struct Manager {
    platform: Platform,
    cache_dir: PathBuf,
    channel: String,
    catalogue_url: String,
}

impl Manager {
    pub fn new() -> Result<Self> {
        let platform = Platform::current().ok_or_else(|| {
            anyhow!(
                "chrome-for-testing has no binary for this platform ({os}/{arch}); \
                 leave NEXO_PLUGIN_BROWSER_AUTO_DOWNLOAD unset on this target",
                os = std::env::consts::OS,
                arch = std::env::consts::ARCH,
            )
        })?;
        let cache_dir = default_cache_dir()?;
        Ok(Self {
            platform,
            cache_dir,
            channel: "Stable".into(),
            catalogue_url: VERSIONS_URL.into(),
        })
    }

    /// Override the cache root. Default lives under the user's
    /// cache dir (`$XDG_CACHE_HOME` on Linux, `~/Library/Caches/`
    /// on macOS, `%LOCALAPPDATA%` on Windows) at
    /// `nexo-plugin-browser/chrome-for-testing/`.
    pub fn with_cache_dir(mut self, dir: PathBuf) -> Self {
        self.cache_dir = dir;
        self
    }

    pub async fn ensure(&self) -> Result<PathBuf> {
        let catalogue = self.fetch_catalogue().await?;
        let channel = catalogue
            .channels
            .get(&self.channel)
            .ok_or_else(|| anyhow!("channel {} not in catalogue", self.channel))?;

        let version = &channel.version;
        let target_dir = self.cache_dir.join(version).join(self.platform.zip_root());
        let target_bin = target_dir.join(self.platform.binary_name());

        if target_bin.exists() {
            debug!(path = %target_bin.display(), "chrome-headless-shell cache hit");
            return Ok(target_bin);
        }

        let entry = channel
            .downloads
            .get("chrome-headless-shell")
            .ok_or_else(|| anyhow!("catalogue missing chrome-headless-shell downloads"))?
            .iter()
            .find(|d| d.platform == self.platform.key())
            .ok_or_else(|| {
                anyhow!(
                    "catalogue has no chrome-headless-shell for platform {}",
                    self.platform.key()
                )
            })?;

        info!(version = %version, url = %entry.url, "downloading chrome-headless-shell");
        fs::create_dir_all(&self.cache_dir).await?;
        let zip_path = self.cache_dir.join(format!(
            "chrome-headless-shell-{}-{}.zip",
            self.platform.key(),
            version
        ));
        download_zip(&entry.url, &zip_path).await?;
        extract_zip(&zip_path, &self.cache_dir.join(version)).await?;
        let _ = fs::remove_file(&zip_path).await;

        if self.platform.needs_chmod() {
            chmod_executable(&target_bin).await?;
        }

        if !target_bin.exists() {
            return Err(anyhow!(
                "extracted, but binary not found at {}",
                target_bin.display()
            ));
        }
        Ok(target_bin)
    }

    async fn fetch_catalogue(&self) -> Result<Catalogue> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let body = client
            .get(&self.catalogue_url)
            .send()
            .await?
            .error_for_status()?
            .json::<Catalogue>()
            .await
            .context("parse chrome-for-testing JSON catalogue")?;
        Ok(body)
    }
}

// ── Catalogue JSON shape (subset we use) ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Catalogue {
    channels: std::collections::BTreeMap<String, Channel>,
}

#[derive(Debug, Deserialize)]
struct Channel {
    version: String,
    downloads: std::collections::BTreeMap<String, Vec<Download>>,
}

#[derive(Debug, Deserialize)]
struct Download {
    platform: String,
    url: String,
}

// ── IO helpers ──────────────────────────────────────────────────────────────

async fn download_zip(url: &str, target: &Path) -> Result<()> {
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    let mut f = fs::File::create(target).await?;
    let total = resp.content_length().unwrap_or(0);
    let mut written = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        f.write_all(&chunk).await?;
        written += chunk.len() as u64;
        if total > 0 && written % (8 * 1024 * 1024) < (chunk.len() as u64) {
            debug!(written, total, "chrome download progress");
        }
    }
    f.flush().await?;
    debug!(bytes = written, "chrome download complete");
    Ok(())
}

async fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let zip_path = zip_path.to_path_buf();
    let dest = dest.to_path_buf();
    // `zip` is blocking-only; run on the blocking pool.
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&zip_path)
            .with_context(|| format!("open zip {}", zip_path.display()))?;
        let mut archive = zip::ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(rel) = entry.enclosed_name() else {
                // Skip absolute or `..` paths — standard zip safety guard.
                warn!(name = %entry.name(), "skipping unsafe zip entry");
                continue;
            };
            let out_path = dest.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out_path)?;
                continue;
            }
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)
                .with_context(|| format!("create {}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out)?;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

async fn chmod_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).await?;
        let mut perms = metadata.permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(path, perms).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn default_cache_dir() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var("NEXO_BROWSER_CACHE") {
        return Ok(PathBuf::from(custom));
    }
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("could not determine user cache dir"))?;
    Ok(base.join("nexo-plugin-browser").join("chrome-for-testing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_honours_env_override() {
        std::env::set_var("NEXO_BROWSER_CACHE", "/tmp/nexo-cft-cache-test");
        let dir = default_cache_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/nexo-cft-cache-test"));
        std::env::remove_var("NEXO_BROWSER_CACHE");
    }
}
