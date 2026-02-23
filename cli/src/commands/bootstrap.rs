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
        || std::ffi::OsString::from("dotfiles.new"),
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
        write!(hex, "{b:02x}").unwrap_or(());
    }
    Ok(hex)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::io::Write;

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
}
