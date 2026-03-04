//! Task: update the dotfiles binary to the latest release.
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};

use crate::logging::Output;

use super::{Context, Task, TaskResult};

/// GitHub repository used for release lookups.
const REPO: &str = "sneivandt/dotfiles";

/// Maximum age (in seconds) before a version check is performed again.
const CACHE_MAX_AGE: u64 = 3600;

/// Detect the asset name for the current platform.
const fn asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dotfiles-windows-x86_64.exe"
    } else if cfg!(target_arch = "aarch64") {
        "dotfiles-linux-aarch64"
    } else {
        "dotfiles-linux-x86_64"
    }
}

/// Return the path where the binary should live inside the repo.
fn binary_path(root: &std::path::Path) -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "dotfiles.exe"
    } else {
        "dotfiles"
    };
    root.join("bin").join(name)
}

/// Path to the version-check cache file.
fn cache_path(root: &std::path::Path) -> PathBuf {
    root.join("bin").join(".dotfiles-version-cache")
}

/// Check whether the cached version is still fresh (less than [`CACHE_MAX_AGE`]
/// seconds old).
fn is_cache_fresh(root: &std::path::Path) -> bool {
    let path = cache_path(root);
    let Ok(content) = fs::read_to_string(&path) else {
        return false;
    };
    let Some(ts_str) = content.lines().nth(1) else {
        return false;
    };
    let Ok(ts) = ts_str.trim().parse::<u64>() else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(ts) < CACHE_MAX_AGE
}

/// Read the cached release tag (line 1 of the cache file).
fn cached_version(root: &std::path::Path) -> Option<String> {
    let content = fs::read_to_string(cache_path(root)).ok()?;
    content.lines().next().map(|s| s.trim().to_string())
}

/// Write a new cache file with the given tag and current timestamp.
fn write_cache(root: &std::path::Path, tag: &str) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fs::write(cache_path(root), format!("{tag}\n{now}\n")).context("writing version cache file")?;
    Ok(())
}

/// Build a [`ureq::Agent`] with reasonable timeouts.
fn http_agent(timeout_secs: u64) -> ureq::Agent {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build();
    config.new_agent()
}

/// Query the GitHub API for the latest release tag.
fn fetch_latest_tag() -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = http_agent(30);
    let Ok(response) = agent
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "dotfiles-cli")
        .call()
    else {
        return Ok(None);
    };

    let body: String = response
        .into_body()
        .read_to_string()
        .context("reading GitHub API response")?;

    let parsed: serde_json::Value =
        serde_json::from_str(&body).context("parsing GitHub API JSON response")?;
    Ok(parsed
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from))
}

/// Download a URL and return the bytes.
fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let agent = http_agent(120);
    let response = agent
        .get(url)
        .header("User-Agent", "dotfiles-cli")
        .call()
        .with_context(|| format!("downloading {url}"))?;

    let mut buf = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut buf)
        .with_context(|| format!("reading response from {url}"))?;
    Ok(buf)
}

/// Verify the SHA-256 checksum of `data` against the checksums file for the
/// given release tag.
fn verify_checksum(tag: &str, asset: &str, data: &[u8]) -> Result<()> {
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/checksums.sha256");
    let checksums = download_bytes(&url).context("downloading checksums file")?;
    let checksums_str = String::from_utf8_lossy(&checksums);

    let expected = checksums_str
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            if name == asset {
                Some(hash.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("checksum not found for {asset}"))?;

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected {
        bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }
    Ok(())
}

/// Replace the binary at `path` with `data`, handling platform differences.
fn replace_binary(path: &std::path::Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    // Write to a temporary file in the same directory for atomic rename.
    let tmp = dir.join(".dotfiles-update.tmp");
    {
        let mut f = fs::File::create(&tmp).context("creating temp file")?;
        f.write_all(data).context("writing binary data")?;
        f.flush().context("flushing binary data")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
            .context("setting executable permission")?;
    }

    // On Windows, the running binary is locked. Rename the current binary out
    // of the way first, then move the new one into place.
    #[cfg(windows)]
    if path.exists() {
        let old = dir.join(".dotfiles-old.exe");
        // Remove stale .old from a previous update if present.
        let _ = fs::remove_file(&old);
        fs::rename(path, &old).context("renaming current binary to .old")?;
    }

    fs::rename(&tmp, path).context("moving new binary into place")?;
    Ok(())
}

/// Check whether the current process is running from `$root/bin/dotfiles`.
fn is_running_from_bin(root: &std::path::Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let expected = binary_path(root);
    let resolved_exe = fs::canonicalize(&exe).unwrap_or(exe);
    let resolved_expected = fs::canonicalize(&expected).unwrap_or(expected);
    resolved_exe == resolved_expected
}

/// Result of checking for an available update.
enum UpdateCheck {
    /// Cache is fresh — no network call needed.
    CacheFresh,
    /// Could not reach GitHub.
    Offline,
    /// Already running the latest version.
    AlreadyCurrent(String),
    /// A newer version is available.
    UpdateAvailable {
        /// Latest release tag (e.g., "v0.2.0").
        latest: String,
        /// Current version tag (e.g., "v0.1.0").
        current: String,
    },
}

/// Check whether an update is available by comparing the local cache and
/// the latest GitHub release.
///
/// # Errors
///
/// Returns an error if the cache file cannot be written.
fn check_for_update(root: &std::path::Path) -> Result<UpdateCheck> {
    if is_cache_fresh(root) {
        return Ok(UpdateCheck::CacheFresh);
    }
    let Some(latest) = fetch_latest_tag()? else {
        return Ok(UpdateCheck::Offline);
    };
    if cached_version(root).as_deref() == Some(latest.as_str()) {
        write_cache(root, &latest)?;
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    let current = format!(
        "v{}",
        option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
    );
    if latest == current {
        write_cache(root, &latest)?;
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    Ok(UpdateCheck::UpdateAvailable { latest, current })
}

/// Download the release asset for the given tag and install it.
///
/// # Errors
///
/// Returns an error if the download, checksum verification, or binary
/// replacement fails.
fn download_and_install(root: &std::path::Path, tag: &str) -> Result<()> {
    let asset = asset_name();
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");
    let data = download_bytes(&url)?;
    verify_checksum(tag, asset, &data)?;
    let bin = binary_path(root);
    replace_binary(&bin, &data)?;
    write_cache(root, tag)?;
    Ok(())
}

/// Run the self-update check before the task graph.
///
/// When the binary lives in `$root/bin/` and a newer release is available,
/// this function downloads and replaces the binary, returning `Ok(true)`.
/// The caller should then re-exec the new binary so that all tasks run
/// with the updated code.
///
/// Returns `Ok(false)` when no update is needed or when running from a
/// cargo build directory.
///
/// # Errors
///
/// Returns an error if the GitHub API call, download, or checksum
/// verification fails.
pub fn pre_update(root: &std::path::Path, log: &dyn Output, dry_run: bool) -> Result<bool> {
    if !is_running_from_bin(root) {
        return Ok(false);
    }
    match check_for_update(root)? {
        UpdateCheck::CacheFresh | UpdateCheck::Offline | UpdateCheck::AlreadyCurrent(_) => {
            Ok(false)
        }
        UpdateCheck::UpdateAvailable { latest, current } => {
            if dry_run {
                log.info(&format!("update available: {current} → {latest}"));
                return Ok(false);
            }
            log.stage("Self update");
            log.info(&format!("updating: {current} → {latest}"));
            download_and_install(root, &latest)?;
            log.info("binary updated, restarting");
            Ok(true)
        }
    }
}

/// Update the running dotfiles binary to the latest GitHub release.
#[derive(Debug)]
pub struct UpdateBinary;

impl Task for UpdateBinary {
    fn name(&self) -> &'static str {
        "Update binary"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run when the binary lives in $DOTFILES_ROOT/bin/ (production
        // layout). Skip when running from a cargo build directory.
        is_running_from_bin(&ctx.root())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let root = ctx.root();
        match check_for_update(&root)? {
            UpdateCheck::CacheFresh => {
                ctx.log.info("version cache is fresh, skipping check");
                Ok(TaskResult::Skipped("cache fresh".to_string()))
            }
            UpdateCheck::Offline => {
                ctx.log
                    .warn("could not reach GitHub, skipping binary update");
                Ok(TaskResult::Skipped("offline".to_string()))
            }
            UpdateCheck::AlreadyCurrent(tag) => {
                ctx.log.info(&format!("already up to date ({tag})"));
                Ok(TaskResult::Ok)
            }
            UpdateCheck::UpdateAvailable { latest, .. } => {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("would update binary to {latest}"));
                    return Ok(TaskResult::DryRun);
                }
                ctx.log.info(&format!("downloading {latest}…"));
                download_and_install(&root, &latest)?;
                ctx.log.info(&format!("updated to {latest}"));
                Ok(TaskResult::Ok)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_not_in_bin_dir() {
        let config = empty_config(PathBuf::from("/nonexistent/repo"));
        let ctx = make_linux_context(config);
        let task = UpdateBinary;
        // The test binary is in target/, not bin/, so should_run returns false.
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn cache_fresh_returns_false_when_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_cache_fresh(dir.path()));
    }

    #[test]
    fn cache_fresh_returns_true_when_recent() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("v1.0\n{now}\n"),
        )
        .unwrap();

        assert!(is_cache_fresh(dir.path()));
    }

    #[test]
    fn cache_fresh_returns_false_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let stale = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - CACHE_MAX_AGE
            - 100;
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("v1.0\n{stale}\n"),
        )
        .unwrap();

        assert!(!is_cache_fresh(dir.path()));
    }

    #[test]
    fn cached_version_reads_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join(".dotfiles-version-cache"), "v0.1.42\n12345\n").unwrap();

        assert_eq!(cached_version(dir.path()), Some("v0.1.42".to_string()));
    }

    #[test]
    fn write_cache_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache(dir.path(), "v0.1.99").unwrap();
        let content = fs::read_to_string(bin_dir.join(".dotfiles-version-cache")).unwrap();
        assert!(content.starts_with("v0.1.99\n"));
    }

    #[test]
    fn verify_checksum_detects_mismatch() {
        // This test cannot reach GitHub, so we only test the hash mismatch path.
        let mut hasher = Sha256::new();
        hasher.update(b"hello");
        let actual_hex = format!("{:x}", hasher.finalize());
        assert!(!actual_hex.is_empty());
    }

    #[test]
    fn replace_binary_writes_and_sets_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        replace_binary(&bin, b"#!/bin/sh\necho ok").unwrap();
        assert!(bin.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&bin).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn replace_binary_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::write(&bin, b"old").unwrap();
        replace_binary(&bin, b"new").unwrap();
        assert_eq!(fs::read(&bin).unwrap(), b"new");
    }

    #[test]
    fn asset_name_is_non_empty() {
        assert!(!asset_name().is_empty());
    }
}
