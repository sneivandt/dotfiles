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

use anyhow::Result;

use crate::logging::Output;
use crate::phases::{Context, Task, TaskPhase, TaskResult};

/// GitHub repository used for release lookups.
pub(super) const REPO: &str = "sneivandt/dotfiles";

use cache::{is_cache_fresh, write_cache};
use http::{HttpClient, default_http_client, fetch_latest_tag};
use install::download_and_install;
use paths::is_running_from_bin;
use version::{is_newer, is_release_version};

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
    write_cache(root, &latest)?;
    if latest == current {
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    if !is_newer(&latest, &current) {
        tracing::debug!("latest release {latest} is not newer than current {current}, skipping");
        return Ok(UpdateCheck::AlreadyCurrent(latest));
    }
    Ok(UpdateCheck::UpdateAvailable { latest, current })
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
        UpdateCheck::CacheFresh
        | UpdateCheck::Offline
        | UpdateCheck::DevBuild
        | UpdateCheck::AlreadyCurrent(_) => Ok(false),
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

/// Update the running dotfiles binary to the latest GitHub release.
#[derive(Debug)]
pub struct UpdateBinary;

impl Task for UpdateBinary {
    fn name(&self) -> &'static str {
        "Update binary"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Bootstrap
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run when the binary lives in $DOTFILES_ROOT/bin/ (production
        // layout). Skip when running from a cargo build directory.
        is_running_from_bin(&ctx.root())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let root = ctx.root();
        let client = default_http_client();
        match check_for_update(&root, &client)? {
            UpdateCheck::CacheFresh => {
                Ok(TaskResult::Skipped("version cache is fresh".to_string()))
            }
            UpdateCheck::Offline => {
                ctx.log
                    .warn("could not reach GitHub, skipping binary update");
                Ok(TaskResult::Skipped("offline".to_string()))
            }
            UpdateCheck::DevBuild => {
                ctx.log.debug("dev build, skipping update check");
                Ok(TaskResult::Skipped("dev build".to_string()))
            }
            UpdateCheck::AlreadyCurrent(tag) => {
                ctx.log.debug(&format!("already up to date ({tag})"));
                Ok(TaskResult::Ok)
            }
            UpdateCheck::UpdateAvailable { latest, .. } => {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("would update binary to {latest}"));
                    return Ok(TaskResult::DryRun);
                }
                ctx.log.info(&format!("downloading {latest}…"));
                download_and_install(&root, &latest, &client)?;
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
    use crate::phases::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_not_in_bin_dir() {
        let config = empty_config(PathBuf::from("/nonexistent/repo"));
        let ctx = make_linux_context(config);
        let task = UpdateBinary;
        // The test binary is in target/, not bin/, so should_run returns false.
        assert!(!task.should_run(&ctx));
    }
}
