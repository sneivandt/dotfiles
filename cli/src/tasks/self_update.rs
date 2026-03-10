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

/// Staging file used on Windows until the wrapper can promote the update.
#[cfg(windows)]
const PENDING_BINARY_NAME: &str = ".dotfiles-update.pending";

/// Metadata file that stores the staged version tag.
#[cfg(windows)]
const PENDING_VERSION_NAME: &str = ".dotfiles-update.version";

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

/// Path to a staged binary update.
#[cfg(windows)]
fn pending_binary_path(root: &std::path::Path) -> PathBuf {
    root.join("bin").join(PENDING_BINARY_NAME)
}

/// Path to the staged version metadata file.
#[cfg(windows)]
fn pending_version_path(root: &std::path::Path) -> PathBuf {
    root.join("bin").join(PENDING_VERSION_NAME)
}

/// Path where the old Unix binary is backed up before an in-place update.
///
/// The file is written by [`replace_binary`] and restored by
/// [`download_and_install`] if the post-install smoke test fails.
#[cfg(unix)]
fn old_binary_path(root: &std::path::Path) -> PathBuf {
    root.join("bin").join(".dotfiles.old")
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
    unix_timestamp().saturating_sub(ts) < CACHE_MAX_AGE
}

/// Write a new cache file with the given tag and current timestamp.
fn write_cache(root: &std::path::Path, tag: &str) -> Result<()> {
    let now = unix_timestamp();
    fs::write(cache_path(root), format!("{tag}\n{now}\n")).context("writing version cache file")?;
    Ok(())
}

/// Return the current UTC time as seconds since the Unix epoch.
///
/// Returns `0` if the system clock is before the epoch (should never happen
/// on a properly configured system).
fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// HTTP abstraction
// ---------------------------------------------------------------------------

/// Trait for making HTTP GET requests, enabling test injection.
///
/// Production code uses [`UreqClient`]; tests inject a mock that returns
/// predetermined responses without touching the network.
trait HttpClient: std::fmt::Debug + Send + Sync {
    /// Perform an HTTP GET request and return the response body as bytes.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures, non-success status codes, or
    /// response-body read errors.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>>;
}

/// Real HTTP client backed by [`ureq`].
#[derive(Debug)]
struct UreqClient {
    /// Global request timeout in seconds.
    timeout_secs: u64,
}

impl UreqClient {
    /// Create a new client with the given timeout.
    const fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(self.timeout_secs)))
            .build();
        let agent = config.new_agent();
        let mut req = agent.get(url);
        for &(k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.call().with_context(|| format!("GET {url}"))?;
        let mut buf = Vec::new();
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut buf)
            .with_context(|| format!("reading response from {url}"))?;
        Ok(buf)
    }
}

/// Build the default HTTP client used by the self-update subsystem.
const fn default_http_client() -> UreqClient {
    UreqClient::new(120)
}

/// Query the GitHub API for the latest release tag.
fn fetch_latest_tag(client: &dyn HttpClient) -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let Ok(body_bytes) = client.get(
        &url,
        &[
            ("Accept", "application/vnd.github.v3+json"),
            ("User-Agent", "dotfiles-cli"),
        ],
    ) else {
        return Ok(None);
    };

    let body = String::from_utf8_lossy(&body_bytes);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).context("parsing GitHub API JSON response")?;
    Ok(parsed
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from))
}

/// Download a URL and return the bytes.
fn download_bytes(client: &dyn HttpClient, url: &str) -> Result<Vec<u8>> {
    client
        .get(url, &[("User-Agent", "dotfiles-cli")])
        .with_context(|| format!("downloading {url}"))
}

/// Verify the SHA-256 checksum of `data` against the checksums file for the
/// given release tag.
fn verify_checksum(client: &dyn HttpClient, tag: &str, asset: &str, data: &[u8]) -> Result<()> {
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/checksums.sha256");
    let checksums = download_bytes(client, &url).context("downloading checksums file")?;
    let checksums_str = String::from_utf8_lossy(&checksums);

    let expected = checksums_str
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let parsed_name = parts.collect::<Vec<_>>().join(" ");
            let stripped_name = parsed_name.strip_prefix('*').unwrap_or(&parsed_name);
            if stripped_name == asset {
                Some(hash.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("checksum not found for {asset}"))?;

    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid checksum format for {asset}: expected 64 hex chars, got '{expected}'");
    }

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = format!("{:x}", hasher.finalize());

    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }
    Ok(())
}

/// Replace the binary at `path` with `data`, handling platform differences.
#[cfg_attr(windows, allow(dead_code))]
fn replace_binary(path: &std::path::Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    // Write to a temporary file in the same directory for atomic rename.
    let tmp_path = dir.join(".dotfiles-update.tmp");
    let mut tmp = crate::fs::TempPath::new(tmp_path);

    {
        let mut f = fs::File::create(tmp.path()).context("creating temp file")?;
        f.write_all(data).context("writing binary data")?;
        f.flush().context("flushing binary data")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755))
            .context("setting executable permission")?;
    }

    if path.is_dir() {
        bail!("binary path points to a directory: {}", path.display());
    }

    // On Windows, the running binary is locked. Rename the current binary out
    // of the way first, then move the new one into place.
    #[cfg(windows)]
    if path.exists() {
        let old = dir.join(".dotfiles-old.exe");
        fs::remove_file(&old).ok(); // Remove stale .old from a previous update if present
        fs::rename(path, &old).context("renaming current binary to .old")?;
    }

    // On Unix the running binary can be overwritten, but we keep a backup
    // so that the smoke test in `download_and_install` can restore it on
    // failure.
    #[cfg(unix)]
    if path.exists() {
        let old = dir.join(".dotfiles.old");
        fs::remove_file(&old).ok(); // Remove stale .old from a previous update if present
        fs::rename(path, &old).context("backing up current binary to .old")?;
    }

    fs::rename(tmp.path(), path).context("moving new binary into place")?;
    tmp.persist();
    Ok(())
}

/// Stage an update for later promotion by the wrapper.
#[cfg(windows)]
fn stage_binary(root: &std::path::Path, tag: &str, data: &[u8]) -> Result<()> {
    let pending = pending_binary_path(root);
    let dir = pending
        .parent()
        .ok_or_else(|| anyhow::anyhow!("pending binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    let tmp_path = dir.join(".dotfiles-update.tmp");
    let mut tmp = crate::fs::TempPath::new(tmp_path);
    {
        let mut f = fs::File::create(tmp.path()).context("creating temp staged file")?;
        f.write_all(data).context("writing staged binary data")?;
        f.flush().context("flushing staged binary data")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755))
            .context("setting staged executable permission")?;
    }

    if pending.exists() {
        fs::remove_file(&pending).context("removing previous staged binary")?;
    }
    fs::rename(tmp.path(), &pending).context("moving staged binary into place")?;
    tmp.persist();
    fs::write(pending_version_path(root), format!("{tag}\n"))
        .context("writing staged update metadata")?;
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
    let matched = resolved_exe == resolved_expected;
    tracing::debug!(
        "is_running_from_bin: resolved_exe={resolved_exe:?} resolved_expected={resolved_expected:?} match={matched}"
    );
    matched
}

/// Return `true` if `v` is a proper release version tag (`vMAJOR.MINOR.PATCH`).
///
/// Development builds produced by `git describe` (e.g., `v0.1.2-3-gabcdef` or
/// `c6c5897-dirty`) are not release versions and must not trigger a self-update.
fn is_release_version(v: &str) -> bool {
    parse_semver(v).is_some()
}

/// Parse a version string into `(major, minor, patch)`.
///
/// Accepts both `vMAJOR.MINOR.PATCH` and `MAJOR.MINOR.PATCH` formats.
/// Returns `None` for development builds, pre-release tags, or malformed input.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let mut parts = v.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Return `true` if `latest` is strictly newer than `current`.
///
/// Both must be valid semver tags; returns `false` if either cannot be parsed.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Result of checking for an available update.
enum UpdateCheck {
    /// Cache is fresh — no network call needed.
    CacheFresh,
    /// Could not reach GitHub.
    Offline,
    /// Already running the latest version.
    AlreadyCurrent(String),
    /// Running a development build; self-update is not applicable.
    DevBuild,
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
/// Only triggers an update when the latest release is strictly newer than the
/// running version (semantic version comparison), preventing silent downgrades.
///
/// # Errors
///
/// Returns an error if the cache file cannot be written.
fn check_for_update(root: &std::path::Path, client: &dyn HttpClient) -> Result<UpdateCheck> {
    let raw_version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    let current = format!("v{}", raw_version.strip_prefix('v').unwrap_or(raw_version));
    if !is_release_version(&current) {
        tracing::debug!("dev build ({current}), skipping update check");
        return Ok(UpdateCheck::DevBuild);
    }
    if is_cache_fresh(root) {
        return Ok(UpdateCheck::CacheFresh);
    }
    let Some(latest) = fetch_latest_tag(client)? else {
        tracing::debug!("fetch_latest_tag returned None, treating as offline");
        return Ok(UpdateCheck::Offline);
    };
    if latest == current {
        write_cache(root, &latest)?;
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    if !is_newer(&latest, &current) {
        tracing::debug!("latest release {latest} is not newer than current {current}, skipping");
        write_cache(root, &latest)?;
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    Ok(UpdateCheck::UpdateAvailable { latest, current })
}

/// Dispatch an [`UpdateCheck`] to the caller-provided handler for that variant.
///
/// This centralises the branching over update-check outcomes so callers only
/// provide the per-variant behaviour they need, keeping `pre_update` and
/// [`UpdateBinary::run`] in sync when new variants are added.
fn handle_update_check<T, FCacheFresh, FOffline, FDevBuild, FAlreadyCurrent, FUpdateAvailable>(
    check: UpdateCheck,
    cache_fresh: FCacheFresh,
    offline: FOffline,
    dev_build: FDevBuild,
    already_current: FAlreadyCurrent,
    update_available: FUpdateAvailable,
) -> Result<T>
where
    FCacheFresh: FnOnce() -> Result<T>,
    FOffline: FnOnce() -> Result<T>,
    FDevBuild: FnOnce() -> Result<T>,
    FAlreadyCurrent: FnOnce(String) -> Result<T>,
    FUpdateAvailable: FnOnce(String, String) -> Result<T>,
{
    match check {
        UpdateCheck::CacheFresh => cache_fresh(),
        UpdateCheck::Offline => offline(),
        UpdateCheck::DevBuild => dev_build(),
        UpdateCheck::AlreadyCurrent(tag) => already_current(tag),
        UpdateCheck::UpdateAvailable { latest, current } => update_available(latest, current),
    }
}

/// Run the binary at `path` with `--version` as a basic sanity check.
///
/// Called immediately after a self-update to verify that the new binary
/// starts correctly.  On failure the caller is expected to restore the
/// backup created by [`replace_binary`].
///
/// # Errors
///
/// Returns an error if the process cannot be spawned or exits with a
/// non-zero status code.
#[cfg(not(windows))]
fn smoke_test_binary(path: &std::path::Path) -> Result<()> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("spawning smoke test for {}", path.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "new binary failed smoke test (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

/// Download the release asset for the given tag and install it.
///
/// # Errors
///
/// Returns an error if the download, checksum verification, binary
/// replacement, or smoke test fails.  On a smoke-test failure the previous
/// binary is restored from the `.dotfiles.old` backup.
fn download_and_install(root: &std::path::Path, tag: &str, client: &dyn HttpClient) -> Result<()> {
    let asset = asset_name();
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");
    let data = download_bytes(client, &url)?;
    verify_checksum(client, tag, asset, &data)?;

    #[cfg(windows)]
    {
        stage_binary(root, tag, &data)?;
        write_cache(root, tag)?;
    }

    #[cfg(not(windows))]
    {
        let bin = binary_path(root);
        replace_binary(&bin, &data)?;
        if let Err(smoke_err) = smoke_test_binary(&bin) {
            let old = old_binary_path(root);
            if old.exists()
                && let Err(restore_err) = fs::rename(&old, &bin)
            {
                tracing::warn!(
                    "CRITICAL: smoke-test failed and automatic rollback also failed ({restore_err:#}). \
                     Manual intervention required: restore {old:?} to {bin:?}"
                );
            }
            return Err(smoke_err);
        }
        write_cache(root, tag)?;
    }

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
    let client = default_http_client();
    handle_update_check(
        check_for_update(root, &client)?,
        || Ok(false),
        || Ok(false),
        || Ok(false),
        |_| Ok(false),
        |latest, current| {
            if dry_run {
                log.info(&format!("update available: {current} → {latest}"));
                return Ok(false);
            }
            log.stage("Self update");
            log.info(&format!("updating: {current} → {latest}"));
            download_and_install(root, &latest, &client)?;

            #[cfg(windows)]
            log.info("binary staged, wrapper restart required");

            #[cfg(not(windows))]
            log.info("binary updated, restarting");

            Ok(true)
        },
    )
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
        let client = default_http_client();
        handle_update_check(
            check_for_update(&root, &client)?,
            || {
                ctx.log.info("version cache is fresh, skipping check");
                Ok(TaskResult::Skipped("cache fresh".to_string()))
            },
            || {
                ctx.log
                    .warn("could not reach GitHub, skipping binary update");
                Ok(TaskResult::Skipped("offline".to_string()))
            },
            || {
                ctx.log.info("dev build, skipping update check");
                Ok(TaskResult::Skipped("dev build".to_string()))
            },
            |tag| {
                ctx.log.info(&format!("already up to date ({tag})"));
                Ok(TaskResult::Ok)
            },
            |latest, _current| {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("would update binary to {latest}"));
                    return Ok(TaskResult::DryRun);
                }
                ctx.log.info(&format!("downloading {latest}…"));
                download_and_install(&root, &latest, &client)?;
                ctx.log.info(&format!("updated to {latest}"));
                Ok(TaskResult::Ok)
            },
        )
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
    fn write_cache_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache(dir.path(), "v0.1.99").unwrap();
        let content = fs::read_to_string(bin_dir.join(".dotfiles-version-cache")).unwrap();
        assert!(content.starts_with("v0.1.99\n"));
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
    fn replace_binary_cleans_up_temp_file_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::create_dir(&bin).unwrap();

        let result = replace_binary(&bin, b"new");
        assert!(result.is_err());
        assert!(!dir.path().join(".dotfiles-update.tmp").exists());
    }

    #[cfg(windows)]
    #[test]
    fn stage_binary_writes_pending_files() {
        let dir = tempfile::tempdir().unwrap();

        stage_binary(dir.path(), "v1.2.3", b"new-binary").unwrap();

        assert_eq!(
            fs::read(pending_binary_path(dir.path())).unwrap(),
            b"new-binary"
        );
        assert_eq!(
            fs::read_to_string(pending_version_path(dir.path())).unwrap(),
            "v1.2.3\n"
        );
    }

    #[test]
    fn asset_name_is_non_empty() {
        assert!(!asset_name().is_empty());
    }

    #[test]
    fn is_release_version_accepts_semver_tags() {
        assert!(is_release_version("v0.1.0"));
        assert!(is_release_version("v1.2.3"));
        assert!(is_release_version("v0.1.163"));
    }

    #[test]
    fn is_release_version_rejects_dev_builds() {
        assert!(!is_release_version("c6c5897-dirty"));
        assert!(!is_release_version("vc6c5897-dirty"));
        assert!(!is_release_version("v0.1.2-3-gabcdef"));
        assert!(!is_release_version("v0.1.2-dirty"));
        assert!(!is_release_version("0.1.2-dirty"));
        assert!(!is_release_version(""));
    }

    #[test]
    fn parse_semver_extracts_version_tuple() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v0.1.163"), Some((0, 1, 163)));
        assert_eq!(parse_semver("not-a-version"), None);
        assert_eq!(parse_semver("v0.1.2-dirty"), None);
        assert_eq!(parse_semver(""), None);
    }

    #[test]
    fn is_newer_compares_semantically() {
        // Newer versions
        assert!(is_newer("v0.2.0", "v0.1.0"));
        assert!(is_newer("v1.0.0", "v0.9.9"));
        assert!(is_newer("v0.1.1", "v0.1.0"));

        // Equal versions
        assert!(!is_newer("v0.1.0", "v0.1.0"));

        // Older versions (would-be downgrade)
        assert!(!is_newer("v0.1.0", "v0.2.0"));
        assert!(!is_newer("v0.9.9", "v1.0.0"));

        // Invalid versions
        assert!(!is_newer("garbage", "v0.1.0"));
        assert!(!is_newer("v0.2.0", "garbage"));
    }

    // -----------------------------------------------------------------------
    // MockHttpClient — deterministic HTTP for unit tests
    // -----------------------------------------------------------------------

    /// A mock HTTP client that returns pre-configured responses from a FIFO queue.
    #[derive(Debug)]
    struct MockHttpClient {
        responses: std::sync::Mutex<std::collections::VecDeque<Result<Vec<u8>>>>,
    }

    impl MockHttpClient {
        /// Create a client that returns the given responses in order.
        fn new(responses: Vec<Result<Vec<u8>>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
            }
        }
    }

    impl HttpClient for MockHttpClient {
        fn get(&self, _url: &str, _headers: &[(&str, &str)]) -> Result<Vec<u8>> {
            self.responses
                .lock()
                .expect("mutex poisoned")
                .pop_front()
                .unwrap_or_else(|| bail!("no more mock responses"))
        }
    }

    // -----------------------------------------------------------------------
    // fetch_latest_tag (with mock HTTP)
    // -----------------------------------------------------------------------

    #[test]
    fn fetch_latest_tag_parses_github_response() {
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec())]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, Some("v1.2.3".to_string()));
    }

    #[test]
    fn fetch_latest_tag_returns_none_on_network_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("network error"))]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn fetch_latest_tag_returns_none_when_tag_name_missing() {
        let client = MockHttpClient::new(vec![Ok(br#"{"name": "Release v1.0"}"#.to_vec())]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // download_bytes (with mock HTTP)
    // -----------------------------------------------------------------------

    #[test]
    fn download_bytes_returns_response_body() {
        let client = MockHttpClient::new(vec![Ok(b"binary data".to_vec())]);
        let result = download_bytes(&client, "https://example.com/file").unwrap();
        assert_eq!(result, b"binary data");
    }

    #[test]
    fn download_bytes_propagates_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("timeout"))]);
        let result = download_bytes(&client, "https://example.com/file");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // verify_checksum (with mock HTTP — full flow)
    // -----------------------------------------------------------------------

    #[test]
    fn verify_checksum_succeeds_with_matching_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());

        let checksums = format!("{hash}  test-asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "test-asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_fails_with_wrong_hash() {
        let checksums =
            "deadbeef0123456789abcdef0123456789abcdef0123456789abcdef01234567  test-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "test-asset", b"hello");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("checksum mismatch"),
            "expected 'checksum mismatch' in: {msg}"
        );
    }

    #[test]
    fn verify_checksum_fails_when_asset_not_in_checksums() {
        let checksums = "abc123  other-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "missing-asset", b"data");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("checksum not found"),
            "expected 'checksum not found' in: {msg}"
        );
    }

    #[test]
    fn verify_checksum_succeeds_when_asset_name_contains_spaces() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());

        let checksums = format!("{hash}  release build/test asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "release build/test asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_succeeds_with_uppercase_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:X}", hasher.finalize());

        let checksums = format!("{hash}  test-asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "test-asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_fails_with_malformed_hash() {
        let checksums = "tooshort  test-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "test-asset", b"data");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("invalid checksum format"),
            "expected 'invalid checksum format' in: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // download_and_install (with mock HTTP — end-to-end)
    // -----------------------------------------------------------------------

    #[test]
    fn download_and_install_writes_verified_binary() {
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let binary_data = b"#!/bin/sh\necho updated";
        let mut hasher = Sha256::new();
        hasher.update(binary_data);
        let hash = format!("{:x}", hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        // Response 1: asset download
        // Response 2: checksums download
        let client =
            MockHttpClient::new(vec![Ok(binary_data.to_vec()), Ok(checksums.into_bytes())]);

        download_and_install(dir.path(), "v1.0.0", &client).unwrap();

        #[cfg(windows)]
        {
            let staged = fs::read(pending_binary_path(dir.path())).unwrap();
            assert_eq!(staged, binary_data);
            assert_eq!(
                fs::read_to_string(pending_version_path(dir.path())).unwrap(),
                "v1.0.0\n"
            );
            let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
            assert!(cache.starts_with("v1.0.0\n"));
        }

        #[cfg(not(windows))]
        {
            let installed = fs::read(binary_path(dir.path())).unwrap();
            assert_eq!(installed, binary_data);
            let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
            assert!(cache.starts_with("v1.0.0\n"));
        }
    }

    // -----------------------------------------------------------------------
    // Unix backup and rollback
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn replace_binary_backs_up_existing_on_unix() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::write(&bin, b"old-content").unwrap();

        replace_binary(&bin, b"new-content").unwrap();

        assert_eq!(fs::read(&bin).unwrap(), b"new-content");
        assert_eq!(
            fs::read(dir.path().join(".dotfiles.old")).unwrap(),
            b"old-content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn smoke_test_binary_passes_for_valid_binary() {
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin = dir.path().join("ok");

        // Copy an existing native binary (true always exits 0 regardless of arguments,
        // including --version) rather than writing a shell script that depends on
        // interpreter execution being permitted in the workspace.
        let true_path = which::which("true").expect("'true' binary not found on PATH");
        fs::copy(&true_path, &bin).expect("failed to copy 'true' binary to temp location");

        let result = smoke_test_binary(&bin);
        assert!(
            result.is_ok(),
            "binary: {}, error: {:?}",
            bin.display(),
            result.unwrap_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn smoke_test_binary_fails_for_bad_binary() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bad");
        fs::write(&bin, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let result = smoke_test_binary(&bin);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("smoke test"),
            "expected 'smoke test' in: {msg}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn download_and_install_restores_on_smoke_test_failure() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // Write an existing binary that exits 0 on --version.
        let old_binary = b"#!/bin/sh\necho v0.9.0\n";
        let bin = binary_path(dir.path());
        fs::write(&bin, old_binary).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        // A "corrupt" binary that always exits 1.
        let bad_binary = b"#!/bin/sh\nexit 1\n";
        let mut hasher = Sha256::new();
        hasher.update(bad_binary);
        let hash = format!("{:x}", hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(bad_binary.to_vec()), Ok(checksums.into_bytes())]);

        let result = download_and_install(dir.path(), "v1.0.0", &client);
        assert!(result.is_err(), "expected smoke-test failure");

        // The old binary must be restored.
        let restored = fs::read(&bin).unwrap();
        assert_eq!(
            restored, old_binary,
            "old binary was not restored after smoke-test failure"
        );

        // Version cache must NOT have been written.
        assert!(
            !cache_path(dir.path()).exists(),
            "cache should not be written after a failed update"
        );
    }
}
