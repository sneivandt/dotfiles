//! Task: update the dotfiles binary to the latest GitHub release.
//!
//! Submodules:
//! - [`attestation`] — GitHub build provenance verification of downloads.
//! - [`paths`]   — binary and cache path helpers.
//! - [`cache`]   — version-check cache I/O.
//! - [`http`]    — HTTP client trait, GitHub API, checksum verification.
//! - [`version`] — date-based release-tag parsing and ordering.
//! - [`install`] — binary replacement, smoke testing, and download.

mod attestation;
mod cache;
mod http;
mod install;
mod paths;
mod version;

/// Lower-case hex encoding of a byte slice.  Used to render SHA-256 digests
/// without pulling in an extra hex crate after `sha2` 0.11 dropped its
/// `LowerHex`/`UpperHex` impls on `Output`.
pub(super) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut acc, b| {
            acc.push(nibble_to_lower(b >> 4));
            acc.push(nibble_to_lower(b & 0x0f));
            acc
        },
    )
}

/// Upper-case hex encoding of a byte slice.  Companion to [`hex_encode`].
#[cfg(test)]
pub(super) fn hex_encode_upper(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut acc, b| {
            acc.push(nibble_to_upper(b >> 4));
            acc.push(nibble_to_upper(b & 0x0f));
            acc
        },
    )
}

fn nibble_to_lower(n: u8) -> char {
    match n & 0x0f {
        digit @ 0..=9 => char::from(b'0'.saturating_add(digit)),
        digit @ 10..=15 => char::from(b'a'.saturating_add(digit).saturating_sub(10)),
        _ => '0',
    }
}

#[cfg(test)]
fn nibble_to_upper(n: u8) -> char {
    match n & 0x0f {
        digit @ 0..=9 => char::from(b'0'.saturating_add(digit)),
        digit @ 10..=15 => char::from(b'A'.saturating_add(digit).saturating_sub(10)),
        _ => '0',
    }
}

use anyhow::Result;

use crate::infra::env::{Env, SystemEnv};
use crate::infra::logging::Output;
use crate::infra::logging::OutputExt as _;
/// GitHub repository used for release lookups.
pub(super) const REPO: &str = "sneivandt/dotfiles";
/// Environment variable that disables the self-update preflight.
const SKIP_SELF_UPDATE_ENV: &str = "DOTFILES_SKIP_SELF_UPDATE";

use cache::{read_fresh_cache, write_cache};
use http::{HttpClient, default_http_client, fetch_latest_tag};
use install::download_and_install;
use paths::is_running_from_bin;
use version::{is_newer, is_release_version};

fn self_update_skipped(env: &dyn Env) -> bool {
    env.var(SKIP_SELF_UPDATE_ENV).as_deref() == Some("1")
}

/// Result of checking for an available update.
enum UpdateCheck {
    /// Could not reach GitHub.
    Offline,
    /// Already running the latest version.
    AlreadyCurrent,
    /// Running a development build; self-update is not applicable.
    DevBuild,
    /// A newer version is available.
    UpdateAvailable {
        /// Latest release tag (e.g., "v2026.07.25-2").
        latest: String,
        /// Current version tag (e.g., "v2026.07.25-1").
        current: String,
    },
}

fn classify_update(current: &str, latest: String) -> UpdateCheck {
    if latest == current {
        return UpdateCheck::AlreadyCurrent;
    }
    if !is_newer(&latest, current) {
        tracing::debug!("latest release {latest} is not newer than current {current}, skipping");
        return UpdateCheck::AlreadyCurrent;
    }
    UpdateCheck::UpdateAvailable {
        latest,
        current: current.to_string(),
    }
}

fn update_check_failure_message(error: &anyhow::Error) -> String {
    let detail = format!("{error:#}");
    if detail.to_ascii_lowercase().contains("http status: 403") {
        return "GitHub denied the anonymous release check (HTTP 403), likely due to API rate limiting; try again later"
            .to_string();
    }
    format!("could not reach GitHub: {detail}")
}

/// Check whether an update is available by comparing the local cache and
/// the latest GitHub release.
///
/// Only triggers an update when the latest release is strictly newer than the
/// running version (date-based tag comparison), preventing silent downgrades.
fn check_for_update(root: &std::path::Path, client: &dyn HttpClient) -> Result<UpdateCheck> {
    let raw_version =
        option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
    check_for_update_with_current(root, client, raw_version)
}

fn check_for_update_with_current(
    root: &std::path::Path,
    client: &dyn HttpClient,
    raw_version: &str,
) -> Result<UpdateCheck> {
    let current = format!("v{}", raw_version.strip_prefix('v').unwrap_or(raw_version));
    if !is_release_version(&current) {
        tracing::debug!("dev build ({current}), skipping update check");
        return Ok(UpdateCheck::DevBuild);
    }
    if let Some(latest) = read_fresh_cache(root)
        && is_release_version(&latest)
    {
        return Ok(classify_update(&current, latest));
    }
    let latest = match fetch_latest_tag(client) {
        Ok(Some(latest)) => latest,
        Ok(None) => {
            tracing::debug!("latest release carried no tag_name, treating as offline");
            return Ok(UpdateCheck::Offline);
        }
        // Self-update runs unattended at the end of `dotfiles update`, so a
        // failed lookup must not fail the run. It must not vanish silently
        // either: without the cause, a persistently broken update (expired
        // proxy, rate limit, DNS) looks identical to being up to date.
        Err(error) => {
            tracing::warn!(
                "skipping self-update: {}",
                update_check_failure_message(&error)
            );
            return Ok(UpdateCheck::Offline);
        }
    };
    let check = classify_update(&current, latest.clone());
    if !matches!(check, UpdateCheck::UpdateAvailable { .. }) {
        write_cache(root, &latest)?;
    }
    Ok(check)
}

/// Run `work` while a transient status line is shown, clearing it afterwards.
///
/// The status describes work the user is waiting on rather than a result, so it
/// must leave no trace once the wait is over.
fn with_status<T>(log: &dyn Output, status: &str, work: impl FnOnce() -> T) -> T {
    log.status_line(status);
    let result = work();
    log.clear_status_line();
    result
}

/// Run the self-update check before the task graph.
///
/// When the binary lives in `$root/bin/` and a newer release is available,
/// this function downloads and replaces the binary, returning `Ok(true)`.
/// The caller should then re-exec the new binary so that all tasks run
/// with the updated code.
///
/// Progress is reported through a transient status line, so a run that was
/// already current leaves no console output at all; only an update that
/// actually happened prints a durable, dimmed line.
///
/// Returns `Ok(false)` when no update is needed or when running from a
/// cargo build directory. Setting `DOTFILES_SKIP_SELF_UPDATE=1` also skips the
/// check without weakening verification for downloads performed by other
/// processes. `skip_attestation` bypasses provenance verification for the
/// downloaded update while preserving checksum verification.
///
/// # Errors
///
/// Returns an error if the GitHub API call, download, or checksum
/// verification fails.
pub fn pre_update(
    root: &std::path::Path,
    log: &dyn Output,
    dry_run: bool,
    skip_attestation: bool,
) -> Result<bool> {
    if self_update_skipped(&SystemEnv) {
        tracing::debug!("self-update skipped by {SKIP_SELF_UPDATE_ENV}");
        return Ok(false);
    }
    if !is_running_from_bin(root) {
        return Ok(false);
    }
    let client = default_http_client();
    let check = with_status(log, "Checking for updates", || {
        check_for_update(root, &client)
    })?;
    match check {
        UpdateCheck::Offline | UpdateCheck::DevBuild | UpdateCheck::AlreadyCurrent => Ok(false),
        UpdateCheck::UpdateAvailable { latest, current } => {
            if dry_run {
                log.info(format!("update available: {current} \u{2192} {latest}"));
                return Ok(false);
            }
            log.stage("Self update");
            log.debug(format!("updating: {current} \u{2192} {latest}"));
            with_status(log, &format!("Updating to {latest}"), || {
                download_and_install(root, &latest, &client, skip_attestation)
            })?;

            log.startup(format!("Self update \u{00b7} {current} \u{2192} {latest}"));

            Ok(true)
        }
    }
}

/// Path of the installed binary inside `root`.
///
/// Exposed so the application layer can re-exec the binary that self-update
/// just replaced without re-deriving the platform-specific file name.
pub fn installed_binary_path(root: &std::path::Path) -> std::path::PathBuf {
    paths::binary_path(root)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::cache::{cache_path, write_cache};
    use super::http::test_support::MockHttpClient;
    use super::*;
    use crate::infra::env::MapEnv;

    #[test]
    fn self_update_skip_requires_explicit_one() {
        assert!(self_update_skipped(
            &MapEnv::new().with(SKIP_SELF_UPDATE_ENV, "1")
        ));
        assert!(!self_update_skipped(&MapEnv::new()));
        assert!(!self_update_skipped(
            &MapEnv::new().with(SKIP_SELF_UPDATE_ENV, "0")
        ));
    }

    #[test]
    fn fresh_cache_newer_than_current_returns_update_available() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        write_cache(dir.path(), "v9999.12.31-1").unwrap();
        let client = MockHttpClient::new(vec![]);

        let result = check_for_update_with_current(dir.path(), &client, "v2026.07.25-1").unwrap();

        match result {
            UpdateCheck::UpdateAvailable { latest, .. } => {
                assert_eq!(latest, "v9999.12.31-1");
            }
            UpdateCheck::Offline | UpdateCheck::AlreadyCurrent | UpdateCheck::DevBuild => {
                panic!("expected cached newer release to trigger update")
            }
        }
    }

    #[test]
    fn network_update_available_does_not_write_cache_before_install() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v9999.12.31-1"}"#.to_vec())]);

        let result = check_for_update_with_current(dir.path(), &client, "v2026.07.25-1").unwrap();

        assert!(matches!(result, UpdateCheck::UpdateAvailable { .. }));
        assert!(
            !cache_path(dir.path()).exists(),
            "cache should only be written after a successful install"
        );
    }

    #[test]
    fn non_newer_network_tag_is_cached_as_latest() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        // GitHub reports an older release than the running binary.
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v2026.07.25-1"}"#.to_vec())]);

        let result = check_for_update_with_current(dir.path(), &client, "v2026.07.25-9").unwrap();

        assert!(matches!(result, UpdateCheck::AlreadyCurrent));
        let cached = fs::read_to_string(cache_path(dir.path())).unwrap();
        assert!(
            cached.starts_with("v2026.07.25-1\n"),
            "the cache holds the latest published tag, not the running version; got {cached:?}"
        );
    }

    #[test]
    fn malformed_fresh_cache_falls_back_to_network() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("bad\n{now}\n"),
        )
        .unwrap();
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v9999.12.31-1"}"#.to_vec())]);

        let result = check_for_update_with_current(dir.path(), &client, "v2026.07.25-1").unwrap();

        assert!(matches!(result, UpdateCheck::UpdateAvailable { .. }));
    }

    #[test]
    fn http_403_failure_explains_anonymous_rate_limiting() {
        let error = anyhow::anyhow!(
            "querying latest release: GET https://api.github.com/repos/example/releases/latest: http status: 403"
        );

        let message = update_check_failure_message(&error);

        assert_eq!(
            message,
            "GitHub denied the anonymous release check (HTTP 403), likely due to API rate limiting; try again later"
        );
    }

    #[test]
    fn other_update_failures_preserve_the_underlying_detail() {
        let error = anyhow::anyhow!("DNS lookup failed");

        let message = update_check_failure_message(&error);

        assert_eq!(message, "could not reach GitHub: DNS lookup failed");
    }
}
