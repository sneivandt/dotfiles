use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::{BootstrapOpts, GlobalOpts};
use crate::exec::Executor;

/// Maximum age of the version cache before a fresh check is needed (seconds).
const CACHE_MAX_AGE: u64 = 3600;

/// Number of download retry attempts.
const RETRY_COUNT: u32 = 3;

/// Seconds to wait between download retries.
const RETRY_DELAY: u64 = 2;

/// TCP connect timeout in seconds.
const CONNECT_TIMEOUT: u64 = 10;

/// Total transfer timeout in seconds.
const TRANSFER_TIMEOUT: u64 = 120;

/// Run the bootstrap command.
///
/// Checks whether the local binary is current and downloads a new one when
/// needed.  The version cache (`bin/.dotfiles-version-cache`) is used to
/// avoid querying GitHub on every invocation.
///
/// # Errors
///
/// Returns an error if the binary cannot be downloaded and no local copy
/// exists, or if a download attempt fails after all retries.
pub fn run(global: &GlobalOpts, opts: &BootstrapOpts, executor: &dyn Executor) -> Result<()> {
    let root = crate::commands::install::resolve_root(global)?;
    let bin_dir = root.join("bin");
    let binary = bin_dir.join(binary_name());
    let cache_file = bin_dir.join(".dotfiles-version-cache");

    // Fast path: binary is present and the version cache is still fresh.
    if !opts.force && binary.exists() && is_cache_fresh(&cache_file)? {
        return Ok(());
    }

    if opts.skip_download {
        return Ok(());
    }

    // Fetch latest release tag from GitHub.
    let latest = fetch_latest_version(&opts.repo, executor);

    if latest.is_empty() {
        // Offline fallback: use whatever binary is already on disk.
        if binary.exists() {
            println!("Using cached dotfiles binary (offline)");
            return Ok(());
        }
        bail!(
            "Cannot determine latest version and no local binary found. \
             Use --build to build from source, or check your internet connection."
        );
    }

    // Compare against the cached release tag.  We intentionally avoid
    // comparing the binary's self-reported version because git-describe
    // output can differ from the release tag.
    let cached = get_cached_version(&cache_file)?;
    if !binary.exists() || cached.as_deref() != Some(latest.as_str()) {
        download_binary(&opts.repo, &latest, &binary, executor)?;
    }

    update_cache(&cache_file, &latest)?;
    Ok(())
}

/// Return the platform-specific binary filename.
const fn binary_name() -> &'static str {
    if cfg!(windows) {
        "dotfiles.exe"
    } else {
        "dotfiles"
    }
}

/// Return the platform- and architecture-specific release asset name.
fn asset_name() -> String {
    if cfg!(windows) {
        "dotfiles-windows-x86_64.exe".to_owned()
    } else {
        let arch = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x86_64"
        };
        format!("dotfiles-linux-{arch}")
    }
}

/// Check whether the version cache is still fresh.
///
/// Returns `false` if the cache file does not exist or its timestamp is older
/// than [`CACHE_MAX_AGE`].
pub fn is_cache_fresh(cache_file: &Path) -> Result<bool> {
    if !cache_file.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(cache_file).context("reading version cache")?;
    let cached_ts: u64 = content
        .lines()
        .nth(1)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(now.saturating_sub(cached_ts) < CACHE_MAX_AGE)
}

/// Return the release tag stored in line 1 of the cache file, if present.
pub fn get_cached_version(cache_file: &Path) -> Result<Option<String>> {
    if !cache_file.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(cache_file).context("reading version cache")?;
    Ok(content.lines().next().map(str::to_owned))
}

/// Write `version` and the current Unix timestamp to the cache file.
pub fn update_cache(cache_file: &Path, version: &str) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Some(parent) = cache_file.parent() {
        std::fs::create_dir_all(parent).context("creating bin directory")?;
    }
    std::fs::write(cache_file, format!("{version}\n{now}\n")).context("writing version cache")
}

/// Fetch the latest release tag from the GitHub releases API.
///
/// Returns an empty string when the network is unavailable or the API call
/// fails (so callers can fall back to an offline binary without treating it
/// as a hard error).
pub fn fetch_latest_version(repo: &str, executor: &dyn Executor) -> String {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let connect_timeout = CONNECT_TIMEOUT.to_string();
    let transfer_timeout = TRANSFER_TIMEOUT.to_string();

    let output = if executor.which("curl") {
        executor
            .run_unchecked(
                "curl",
                &[
                    "-fsSL",
                    "--connect-timeout",
                    &connect_timeout,
                    "--max-time",
                    &transfer_timeout,
                    &url,
                ],
            )
            .ok()
    } else if executor.which("wget") {
        executor
            .run_unchecked(
                "wget",
                &[
                    "-qO-",
                    &format!("--connect-timeout={connect_timeout}"),
                    &format!("--timeout={transfer_timeout}"),
                    &url,
                ],
            )
            .ok()
    } else {
        None
    };

    output
        .filter(|r| r.success)
        .and_then(|r| parse_tag_name(&r.stdout))
        .unwrap_or_default()
}

/// Extract the `tag_name` value from a GitHub releases API JSON response.
///
/// Uses simple string scanning instead of a JSON parser to avoid a
/// `serde_json` dependency.
#[must_use]
pub fn parse_tag_name(json: &str) -> Option<String> {
    let line = json.lines().find(|l| l.contains("\"tag_name\""))?;
    // Expected format:  "tag_name": "v1.2.3",
    let after_key = line.split_once("\"tag_name\"")?.1;
    let after_colon = after_key.split_once(':')?.1.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let inner = after_colon.get(1..)?;
    let end = inner.find('"')?;
    Some(inner[..end].to_owned())
}

/// Download the release binary with retry logic, verify its checksum, and
/// install it atomically.
fn download_binary(
    repo: &str,
    version: &str,
    binary: &Path,
    executor: &dyn Executor,
) -> Result<()> {
    let asset = asset_name();
    let url = format!("https://github.com/{repo}/releases/download/{version}/{asset}");
    let connect_timeout = CONNECT_TIMEOUT.to_string();
    let transfer_timeout = TRANSFER_TIMEOUT.to_string();

    if let Some(parent) = binary.parent() {
        std::fs::create_dir_all(parent).context("creating bin directory")?;
    }

    // Download to a temporary path so an interrupted write does not leave a
    // corrupt binary at the final location.
    let tmp = tmp_path(binary);
    let tmp_str = tmp.to_str().context("tmp path is not valid UTF-8")?;

    println!("Downloading dotfiles {version}...");

    let mut downloaded = false;
    for attempt in 1..=RETRY_COUNT {
        if attempt > 1 {
            println!("Retry {attempt}/{RETRY_COUNT} after {RETRY_DELAY}s...");
            std::thread::sleep(std::time::Duration::from_secs(RETRY_DELAY));
        }

        let result = if executor.which("curl") {
            executor.run_unchecked(
                "curl",
                &[
                    "-fsSL",
                    "--connect-timeout",
                    &connect_timeout,
                    "--max-time",
                    &transfer_timeout,
                    "-o",
                    tmp_str,
                    &url,
                ],
            )
        } else if executor.which("wget") {
            executor.run_unchecked(
                "wget",
                &[
                    "-qO",
                    tmp_str,
                    &format!("--connect-timeout={connect_timeout}"),
                    &format!("--timeout={transfer_timeout}"),
                    &url,
                ],
            )
        } else {
            let _ = std::fs::remove_file(&tmp);
            bail!("curl or wget is required to download the dotfiles binary");
        };

        if result.map(|r| r.success).unwrap_or(false) {
            downloaded = true;
            break;
        }
    }

    if !downloaded {
        let _ = std::fs::remove_file(&tmp);
        bail!(
            "Failed to download dotfiles {version} after {RETRY_COUNT} attempts. \
             Check your internet connection or use --build to build from source."
        );
    }

    verify_checksum(repo, version, &asset, &tmp, executor)?;

    install_binary(&tmp, binary)?;

    Ok(())
}

/// Compute the path used for the temporary download file.
fn tmp_path(binary: &Path) -> PathBuf {
    let mut p = binary.to_path_buf();
    let name = binary.file_name().map_or_else(
        || {
            let mut s = std::ffi::OsString::from(binary_name());
            s.push(".new");
            s
        },
        |n| {
            let mut s = n.to_os_string();
            s.push(".new");
            s
        },
    );
    p.set_file_name(name);
    p
}

/// Move the downloaded binary from `tmp` to `binary`.
///
/// On Unix the rename is atomic.  On Windows it may fail if the target is
/// locked; we leave the `.new` file in place so the shell wrapper can finish
/// the swap after the process exits.
fn install_binary(tmp: &Path, binary: &Path) -> Result<()> {
    match std::fs::rename(tmp, binary) {
        Ok(()) => {}
        Err(e) => {
            if cfg!(windows) {
                // The running executable is locked on Windows.  The wrapper
                // script will complete the rename once this process exits.
                eprintln!(
                    "Note: a binary update is staged at {}; \
                     it will be applied on the next run.",
                    tmp.display()
                );
            } else {
                return Err(anyhow::Error::new(e).context("installing downloaded binary"));
            }
        }
    }
    // Make the binary executable on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(binary) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(binary, perms).context("setting binary permissions")?;
        }
    }
    Ok(())
}

/// Download the checksums file and verify that `binary` matches.
///
/// Skips verification (with a warning) if the checksums file is unavailable
/// or does not contain an entry for this asset.
fn verify_checksum(
    repo: &str,
    version: &str,
    asset: &str,
    binary: &Path,
    executor: &dyn Executor,
) -> Result<()> {
    let url = format!("https://github.com/{repo}/releases/download/{version}/checksums.sha256");
    let connect_timeout = CONNECT_TIMEOUT.to_string();
    let transfer_timeout = TRANSFER_TIMEOUT.to_string();

    let output = if executor.which("curl") {
        executor
            .run_unchecked(
                "curl",
                &[
                    "-fsSL",
                    "--connect-timeout",
                    &connect_timeout,
                    "--max-time",
                    &transfer_timeout,
                    &url,
                ],
            )
            .ok()
    } else if executor.which("wget") {
        executor
            .run_unchecked(
                "wget",
                &[
                    "-qO-",
                    &format!("--connect-timeout={connect_timeout}"),
                    &format!("--timeout={transfer_timeout}"),
                    &url,
                ],
            )
            .ok()
    } else {
        None
    };

    let checksums = if let Some(r) = output.filter(|r| r.success) {
        r.stdout
    } else {
        eprintln!("WARNING: Could not download checksums file; skipping verification");
        return Ok(());
    };

    let expected = checksums
        .lines()
        .find(|line| line.contains(asset))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned);

    let expected = match expected {
        Some(e) if !e.is_empty() => e,
        _ => return Ok(()), // No entry for this asset; skip.
    };

    let actual = compute_sha256(binary)?;
    if expected != actual {
        let _ = std::fs::remove_file(binary);
        bail!("Checksum verification failed! Expected {expected}, got {actual}");
    }

    Ok(())
}

/// Compute the lowercase hex SHA-256 digest of the file at `path`.
pub fn compute_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let bytes = std::fs::read(path).context("reading binary for checksum verification")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in &result {
        // write! to a String is infallible; unwrap_or(()) makes that explicit.
        write!(hex, "{b:02x}").unwrap_or(());
    }
    Ok(hex)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unimplemented
)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    // -----------------------------------------------------------------------
    // Mock executor
    // -----------------------------------------------------------------------

    /// Minimal executor that returns pre-configured responses, used to test
    /// functions that call curl/wget without making real network requests.
    #[derive(Debug, Default)]
    struct MockExecutor {
        /// Maps program name to whether it is available on PATH.
        which_results: HashMap<String, bool>,
        /// Maps program name to (`stdout`, `success`) returned by `run_unchecked`.
        run_results: HashMap<String, (String, bool)>,
    }

    impl MockExecutor {
        fn with_curl(mut self, stdout: &str, success: bool) -> Self {
            self.which_results.insert("curl".to_owned(), true);
            self.run_results
                .insert("curl".to_owned(), (stdout.to_owned(), success));
            self
        }

        fn with_wget(mut self, stdout: &str, success: bool) -> Self {
            self.which_results.insert("wget".to_owned(), true);
            self.run_results
                .insert("wget".to_owned(), (stdout.to_owned(), success));
            self
        }
    }

    impl crate::exec::Executor for MockExecutor {
        fn run(&self, program: &str, _: &[&str]) -> anyhow::Result<crate::exec::ExecResult> {
            anyhow::bail!("MockExecutor::run not implemented ({program})")
        }
        fn run_in(
            &self,
            _: &std::path::Path,
            program: &str,
            _: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            anyhow::bail!("MockExecutor::run_in not implemented ({program})")
        }
        fn run_in_with_env(
            &self,
            _: &std::path::Path,
            program: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            anyhow::bail!("MockExecutor::run_in_with_env not implemented ({program})")
        }
        fn run_unchecked(
            &self,
            program: &str,
            _args: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            let (stdout, success) = self.run_results.get(program).cloned().unwrap_or_default();
            Ok(crate::exec::ExecResult {
                stdout,
                stderr: String::new(),
                success,
                code: Some(i32::from(!success)),
            })
        }
        fn which(&self, program: &str) -> bool {
            self.which_results.get(program).copied().unwrap_or(false)
        }
    }

    // -----------------------------------------------------------------------
    // parse_tag_name
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tag_name_typical_response() {
        let json = r#"{
  "url": "https://api.github.com/repos/owner/repo/releases/1",
  "tag_name": "v0.1.72",
  "name": "v0.1.72"
}"#;
        assert_eq!(parse_tag_name(json), Some("v0.1.72".to_owned()));
    }

    #[test]
    fn parse_tag_name_inline_response() {
        let json = r#"{"id":1,"tag_name":"v1.0.0","name":"Release"}"#;
        assert_eq!(parse_tag_name(json), Some("v1.0.0".to_owned()));
    }

    #[test]
    fn parse_tag_name_missing_field() {
        let json = r#"{"id":1,"name":"Release"}"#;
        assert_eq!(parse_tag_name(json), None);
    }

    #[test]
    fn parse_tag_name_empty_input() {
        assert_eq!(parse_tag_name(""), None);
    }

    #[test]
    fn parse_tag_name_null_value() {
        // `tag_name` present but set to JSON null (no surrounding quotes).
        let json = r#"{"id":1,"tag_name":null,"name":"Release"}"#;
        assert_eq!(parse_tag_name(json), None);
    }

    #[test]
    fn parse_tag_name_pre_release_tag() {
        let json = r#"{"tag_name":"v2.0.0-beta.1"}"#;
        assert_eq!(parse_tag_name(json), Some("v2.0.0-beta.1".to_owned()));
    }

    #[test]
    fn parse_tag_name_multiline_realistic() {
        // Matches a realistic GitHub API response snippet.
        let json = "{\n  \"id\": 123,\n  \"tag_name\": \"v0.2.0\",\n  \"draft\": false\n}";
        assert_eq!(parse_tag_name(json), Some("v0.2.0".to_owned()));
    }

    // -----------------------------------------------------------------------
    // is_cache_fresh / get_cached_version / update_cache
    // -----------------------------------------------------------------------

    #[test]
    fn cache_stale_when_file_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        assert!(!is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn cache_stale_when_timestamp_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        std::fs::write(&cache, "v0.1.0\n0\n").expect("write");
        assert!(!is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn cache_stale_when_no_timestamp_line() {
        // File has only the version line, no timestamp.
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        std::fs::write(&cache, "v1.0.0\n").expect("write");
        assert!(!is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn cache_stale_when_old_timestamp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        let old_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(CACHE_MAX_AGE + 60);
        std::fs::write(&cache, format!("v1.0.0\n{old_ts}\n")).expect("write");
        assert!(!is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn cache_fresh_when_recent_timestamp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        let recent_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(60); // 1 minute ago
        std::fs::write(&cache, format!("v1.0.0\n{recent_ts}\n")).expect("write");
        assert!(is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn cache_fresh_after_update() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        update_cache(&cache, "v0.1.72").expect("update_cache");
        assert!(is_cache_fresh(&cache).expect("is_cache_fresh"));
    }

    #[test]
    fn get_cached_version_after_update() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        update_cache(&cache, "v0.1.72").expect("update_cache");
        assert_eq!(
            get_cached_version(&cache).expect("get_cached_version"),
            Some("v0.1.72".to_owned())
        );
    }

    #[test]
    fn get_cached_version_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        assert_eq!(
            get_cached_version(&cache).expect("get_cached_version"),
            None
        );
    }

    #[test]
    fn get_cached_version_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join(".dotfiles-version-cache");
        std::fs::write(&cache, "").expect("write");
        assert_eq!(
            get_cached_version(&cache).expect("get_cached_version"),
            None
        );
    }

    #[test]
    fn update_cache_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join("deeply/nested/cache");
        update_cache(&cache, "v1.0.0").expect("update_cache");
        assert!(cache.exists(), "cache file should be created");
        assert_eq!(
            get_cached_version(&cache).expect("get_cached_version"),
            Some("v1.0.0".to_owned())
        );
    }

    // -----------------------------------------------------------------------
    // fetch_latest_version (via MockExecutor)
    // -----------------------------------------------------------------------

    const RELEASE_JSON: &str = r#"{"id":1,"tag_name":"v0.1.99","name":"Release v0.1.99"}"#;

    #[test]
    fn fetch_version_uses_curl() {
        let executor = MockExecutor::default().with_curl(RELEASE_JSON, true);
        let version = fetch_latest_version("owner/repo", &executor);
        assert_eq!(version, "v0.1.99");
    }

    #[test]
    fn fetch_version_falls_back_to_wget() {
        let executor = MockExecutor::default().with_wget(RELEASE_JSON, true);
        let version = fetch_latest_version("owner/repo", &executor);
        assert_eq!(version, "v0.1.99");
    }

    #[test]
    fn fetch_version_empty_when_no_tool() {
        let executor = MockExecutor::default();
        let version = fetch_latest_version("owner/repo", &executor);
        assert!(version.is_empty());
    }

    #[test]
    fn fetch_version_empty_on_curl_failure() {
        let executor = MockExecutor::default().with_curl("error", false);
        let version = fetch_latest_version("owner/repo", &executor);
        assert!(version.is_empty());
    }

    #[test]
    fn fetch_version_empty_on_invalid_response() {
        let executor = MockExecutor::default().with_curl("not json at all", true);
        let version = fetch_latest_version("owner/repo", &executor);
        assert!(version.is_empty());
    }

    #[test]
    fn fetch_version_prefers_curl_over_wget() {
        // Both curl (with correct tag) and wget (with different tag) are available.
        let executor = MockExecutor::default()
            .with_curl(RELEASE_JSON, true)
            .with_wget(r#"{"tag_name":"v0.0.1"}"#, true);
        let version = fetch_latest_version("owner/repo", &executor);
        assert_eq!(version, "v0.1.99", "should prefer curl response");
    }

    // -----------------------------------------------------------------------
    // verify_checksum (via MockExecutor)
    // -----------------------------------------------------------------------

    #[test]
    fn verify_checksum_passes_with_matching_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&binary, b"fake binary data").expect("write");
        let hash = compute_sha256(&binary).expect("compute_sha256");
        let asset = "dotfiles-linux-x86_64";
        let checksums_content = format!("{hash}  {asset}\n");
        let executor = MockExecutor::default().with_curl(&checksums_content, true);
        verify_checksum("owner/repo", "v1.0.0", asset, &binary, &executor)
            .expect("checksum should pass with matching hash");
        assert!(
            binary.exists(),
            "binary should remain after passing verification"
        );
    }

    #[test]
    fn verify_checksum_fails_with_wrong_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&binary, b"fake binary data").expect("write");
        let asset = "dotfiles-linux-x86_64";
        let wrong_hash = "a".repeat(64);
        let checksums_content = format!("{wrong_hash}  {asset}\n");
        let executor = MockExecutor::default().with_curl(&checksums_content, true);
        let result = verify_checksum("owner/repo", "v1.0.0", asset, &binary, &executor);
        assert!(
            result.is_err(),
            "should return an error when hash mismatches"
        );
        assert!(
            !binary.exists(),
            "binary should be removed after failed checksum"
        );
    }

    #[test]
    fn verify_checksum_skips_when_asset_not_in_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&binary, b"fake binary data").expect("write");
        let asset = "dotfiles-linux-x86_64";
        // Checksums file mentions a different asset.
        let checksums_content = "aabbcc  dotfiles-linux-aarch64\n";
        let executor = MockExecutor::default().with_curl(checksums_content, true);
        verify_checksum("owner/repo", "v1.0.0", asset, &binary, &executor)
            .expect("should silently skip when asset not found in checksums");
        assert!(binary.exists(), "binary should be untouched");
    }

    #[test]
    fn verify_checksum_skips_when_download_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&binary, b"fake binary data").expect("write");
        let asset = "dotfiles-linux-x86_64";
        let executor = MockExecutor::default().with_curl("404 not found", false);
        verify_checksum("owner/repo", "v1.0.0", asset, &binary, &executor)
            .expect("should silently skip when checksums download fails");
        assert!(binary.exists(), "binary should be untouched");
    }

    #[test]
    fn verify_checksum_skips_when_no_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&binary, b"fake binary data").expect("write");
        let asset = "dotfiles-linux-x86_64";
        let executor = MockExecutor::default(); // no curl, no wget
        verify_checksum("owner/repo", "v1.0.0", asset, &binary, &executor)
            .expect("should silently skip when no download tool available");
        assert!(binary.exists(), "binary should be untouched");
    }

    // -----------------------------------------------------------------------
    // tmp_path
    // -----------------------------------------------------------------------

    #[test]
    fn tmp_path_appends_new_suffix() {
        let binary = PathBuf::from("/some/dir/dotfiles");
        let tmp = tmp_path(&binary);
        assert_eq!(tmp.file_name().unwrap().to_str().unwrap(), "dotfiles.new");
        assert_eq!(tmp.parent(), binary.parent());
    }

    #[test]
    fn tmp_path_preserves_exe_extension() {
        // On Windows the binary is dotfiles.exe; .new should be appended after.
        let binary = PathBuf::from("/some/dir/dotfiles.exe");
        let tmp = tmp_path(&binary);
        assert_eq!(
            tmp.file_name().unwrap().to_str().unwrap(),
            "dotfiles.exe.new"
        );
    }

    // -----------------------------------------------------------------------
    // install_binary
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(unix)]
    fn install_binary_renames_and_sets_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let tmp = dir.path().join("dotfiles.new");
        let binary = dir.path().join("dotfiles");
        std::fs::write(&tmp, b"fake binary content").expect("write tmp");
        install_binary(&tmp, &binary).expect("install_binary");
        assert!(binary.exists(), "binary should exist after install");
        assert!(!tmp.exists(), "tmp file should be gone after rename");
        let mode = std::fs::metadata(&binary)
            .expect("metadata")
            .permissions()
            .mode();
        assert!(mode & 0o100 != 0, "binary should have executable bit set");
    }

    // -----------------------------------------------------------------------
    // compute_sha256
    // -----------------------------------------------------------------------

    #[test]
    fn sha256_known_value() {
        // SHA-256 of the empty string.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("empty");
        std::fs::write(&file, b"").expect("write");
        let hash = compute_sha256(&file).expect("compute_sha256");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_known_content() {
        // echo -n "hello world" | sha256sum
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("hello");
        let mut f = std::fs::File::create(&file).expect("create");
        f.write_all(b"hello world").expect("write_all");
        let hash = compute_sha256(&file).expect("compute_sha256");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_produces_64_hex_chars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("data");
        std::fs::write(&file, b"some content").expect("write");
        let hash = compute_sha256(&file).expect("compute_sha256");
        assert_eq!(hash.len(), 64, "SHA-256 hex digest should be 64 characters");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "digest should contain only hex characters"
        );
    }

    // -----------------------------------------------------------------------
    // asset_name / binary_name helpers
    // -----------------------------------------------------------------------

    #[test]
    fn asset_name_is_nonempty() {
        assert!(!asset_name().is_empty());
    }

    #[test]
    fn binary_name_is_nonempty() {
        assert!(!binary_name().is_empty());
    }

    #[test]
    fn asset_name_contains_arch() {
        let name = asset_name();
        assert!(
            name.contains("x86_64") || name.contains("aarch64"),
            "asset name should contain architecture: {name}"
        );
    }

    #[test]
    fn asset_name_contains_platform() {
        let name = asset_name();
        assert!(
            name.contains("linux") || name.contains("windows"),
            "asset name should contain platform: {name}"
        );
    }
}
