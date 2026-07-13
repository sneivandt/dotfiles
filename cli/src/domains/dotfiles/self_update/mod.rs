//! Task: update the dotfiles binary to the latest GitHub release.
//!
//! Submodules:
//! - [`paths`]   — binary, cache, and staging path helpers.
//! - [`cache`]   — version-check cache I/O.
//! - [`http`]    — HTTP client trait, GitHub API, checksum verification.
//! - [`version`] — semver parsing and ordering for release tags.
//! - [`install`] — binary replacement, staging, smoke testing, and download.

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
        0..=9 => char::from(b'0'.saturating_add(n)),
        10..=15 => char::from(b'a'.saturating_add(n).saturating_sub(10)),
        _ => '0',
    }
}

#[cfg(test)]
fn nibble_to_upper(n: u8) -> char {
    match n & 0x0f {
        0..=9 => char::from(b'0'.saturating_add(n)),
        10..=15 => char::from(b'A'.saturating_add(n).saturating_sub(10)),
        _ => '0',
    }
}

use anyhow::Result;

use crate::runtime::logging::Output;
/// GitHub repository used for release lookups.
pub(super) const REPO: &str = "sneivandt/dotfiles";

use cache::{read_fresh_cache, write_cache};
use http::{HttpClient, default_http_client, fetch_latest_tag};
use install::download_and_install;
use paths::is_running_from_bin;
use version::{is_newer, is_release_version};

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
        /// Latest release tag (e.g., "v0.2.0").
        latest: String,
        /// Current version tag (e.g., "v0.1.0").
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

/// Check whether an update is available by comparing the local cache and
/// the latest GitHub release.
///
/// Only triggers an update when the latest release is strictly newer than the
/// running version (semantic version comparison), preventing silent downgrades.
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
    let Some(latest) = fetch_latest_tag(client)? else {
        tracing::debug!("fetch_latest_tag returned None, treating as offline");
        return Ok(UpdateCheck::Offline);
    };
    let check = classify_update(&current, latest);
    if !matches!(check, UpdateCheck::UpdateAvailable { .. }) {
        write_cache(root, &current)?;
    }
    Ok(check)
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
    match check_for_update(root, &client)? {
        UpdateCheck::Offline | UpdateCheck::DevBuild | UpdateCheck::AlreadyCurrent => Ok(false),
        UpdateCheck::UpdateAvailable { latest, current } => {
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
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use std::fs;

    use super::cache::{cache_path, write_cache};
    use super::http::test_support::MockHttpClient;
    use super::*;

    #[test]
    fn fresh_cache_newer_than_current_returns_update_available() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        write_cache(dir.path(), "v9999.0.0").unwrap();
        let client = MockHttpClient::new(vec![]);

        let result = check_for_update_with_current(dir.path(), &client, "v0.1.0").unwrap();

        match result {
            UpdateCheck::UpdateAvailable { latest, .. } => {
                assert_eq!(latest, "v9999.0.0");
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
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v9999.0.0"}"#.to_vec())]);

        let result = check_for_update_with_current(dir.path(), &client, "v0.1.0").unwrap();

        assert!(matches!(result, UpdateCheck::UpdateAvailable { .. }));
        assert!(
            !cache_path(dir.path()).exists(),
            "cache should only be written after a successful install"
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
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v9999.0.0"}"#.to_vec())]);

        let result = check_for_update_with_current(dir.path(), &client, "v0.1.0").unwrap();

        assert!(matches!(result, UpdateCheck::UpdateAvailable { .. }));
    }
}
