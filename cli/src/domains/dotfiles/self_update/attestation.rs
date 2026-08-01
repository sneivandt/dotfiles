//! GitHub build provenance verification for downloaded release assets.
//!
//! Release assets are attested by the release workflow with
//! `actions/attest-build-provenance`. After the SHA-256 checksum matches, the
//! downloaded bytes are additionally checked against that attestation using the
//! `gh` CLI.
//!
//! Verification is advisory by default: the checksum has already been verified,
//! so a missing or unauthenticated `gh` only produces a warning. The policy is
//! controlled by two environment variables:
//!
//! - `DOTFILES_SKIP_ATTESTATION=1` — skip the check entirely.
//! - `DOTFILES_REQUIRE_ATTESTATION=1` — treat any unverified download as fatal.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::REPO;

/// Environment variable that disables provenance verification.
const SKIP_ENV: &str = "DOTFILES_SKIP_ATTESTATION";

/// Environment variable that makes provenance verification mandatory.
const REQUIRE_ENV: &str = "DOTFILES_REQUIRE_ATTESTATION";

/// How strictly provenance verification is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Policy {
    /// Do not verify provenance at all.
    Skip,
    /// Verify when possible; warn but continue otherwise.
    Advisory,
    /// Verification must succeed.
    Required,
}

/// Resolve the policy from the two supported flag values.
fn policy_from_flags(skip: Option<&str>, require: Option<&str>) -> Policy {
    if skip == Some("1") {
        return Policy::Skip;
    }
    if require == Some("1") {
        return Policy::Required;
    }
    Policy::Advisory
}

/// Resolve the policy from the process environment.
pub(super) fn policy_from_env() -> Policy {
    let skip = std::env::var(SKIP_ENV).ok();
    let require = std::env::var(REQUIRE_ENV).ok();
    policy_from_flags(skip.as_deref(), require.as_deref())
}

/// Abstraction over the `gh` CLI, enabling test injection.
pub(super) trait GhCli: std::fmt::Debug + Send + Sync {
    /// Whether the `gh` CLI is available on `PATH`.
    fn available(&self) -> bool;

    /// Verify the build provenance of the file at `path` for `repo`.
    ///
    /// Returns `Ok(true)` when `gh` reports a verified attestation.
    ///
    /// # Errors
    ///
    /// Returns an error only when `gh` cannot be executed at all.
    fn verify(&self, path: &Path, repo: &str) -> Result<bool>;
}

/// Production [`GhCli`] backed by the `gh` executable on `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SystemGh;

impl GhCli for SystemGh {
    fn available(&self) -> bool {
        which::which("gh").is_ok()
    }

    fn verify(&self, path: &Path, repo: &str) -> Result<bool> {
        let path_arg = path.display().to_string();
        let result = crate::infra::exec::run_tool_unchecked(
            "gh",
            &["attestation", "verify", &path_arg, "--repo", repo],
        )?;
        if !result.success {
            tracing::debug!(
                "gh attestation verify failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            );
        }
        Ok(result.success)
    }
}

/// Verify the provenance of downloaded release bytes.
///
/// The bytes are staged in a temporary file because `gh` verifies a file on
/// disk.
///
/// # Errors
///
/// Returns an error when the policy is [`Policy::Required`] and provenance
/// cannot be verified, or when the temporary file cannot be written.
pub(super) fn verify_provenance(
    gh: &dyn GhCli,
    policy: Policy,
    asset: &str,
    data: &[u8],
) -> Result<()> {
    if policy == Policy::Skip {
        tracing::debug!("provenance verification skipped by {SKIP_ENV}");
        return Ok(());
    }

    if !gh.available() {
        return unverified(
            policy,
            asset,
            "gh CLI not found; cannot verify build provenance",
        );
    }

    let staged = stage_for_verification(asset, data)?;
    let verified = match gh.verify(staged.path(), REPO) {
        Ok(verified) => verified,
        Err(error) => {
            return unverified(
                policy,
                asset,
                &format!("gh could not be executed: {error:#}"),
            );
        }
    };

    if verified {
        tracing::debug!("verified build provenance for {asset}");
        return Ok(());
    }

    unverified(policy, asset, "gh reported no verified attestation")
}

/// Apply the policy to an asset whose provenance could not be verified.
fn unverified(policy: Policy, asset: &str, reason: &str) -> Result<()> {
    if policy == Policy::Required {
        bail!("build provenance verification failed for {asset}: {reason}");
    }
    tracing::warn!(
        "could not verify build provenance for {asset}: {reason}. \
         The SHA-256 checksum matched the published release."
    );
    Ok(())
}

/// Write `data` to a uniquely named temporary file for verification.
///
/// The counter keeps concurrent calls from colliding. The PID alone is not
/// enough: unit tests exercise this path from several threads of one process
/// with the same asset name, so a shared path let one call's [`TempGuard`] guard
/// delete a file another call was still writing.
///
/// [`TempGuard`]: crate::infra::fs::TempGuard
fn stage_for_verification(asset: &str, data: &[u8]) -> Result<crate::infra::fs::TempGuard> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let unique = format!(
        ".dotfiles-attest-{pid}-{seq}-{asset}",
        pid = std::process::id(),
        seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        asset = asset
    );
    let path = std::env::temp_dir().join(unique);
    let temp = crate::infra::fs::TempGuard::file(path);
    std::fs::write(temp.path(), data).with_context(|| {
        format!(
            "writing {asset} to {} for provenance verification",
            temp.path().display()
        )
    })?;
    Ok(temp)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct StubGh {
        available: bool,
        result: bool,
        fails_to_run: bool,
    }

    impl GhCli for StubGh {
        fn available(&self) -> bool {
            self.available
        }

        fn verify(&self, _path: &Path, _repo: &str) -> Result<bool> {
            if self.fails_to_run {
                bail!("gh exploded");
            }
            Ok(self.result)
        }
    }

    const fn stub(available: bool, result: bool) -> StubGh {
        StubGh {
            available,
            result,
            fails_to_run: false,
        }
    }

    #[test]
    fn skip_flag_wins_over_require_flag() {
        assert_eq!(policy_from_flags(Some("1"), Some("1")), Policy::Skip);
    }

    #[test]
    fn require_flag_selects_required_policy() {
        assert_eq!(policy_from_flags(None, Some("1")), Policy::Required);
    }

    #[test]
    fn unset_flags_select_advisory_policy() {
        assert_eq!(policy_from_flags(None, None), Policy::Advisory);
        assert_eq!(policy_from_flags(Some("0"), Some("0")), Policy::Advisory);
    }

    #[test]
    fn skip_policy_does_not_invoke_gh() {
        let gh = stub(false, false);
        verify_provenance(&gh, Policy::Skip, "dotfiles-linux-x86_64", b"data").unwrap();
    }

    #[test]
    fn advisory_policy_tolerates_missing_gh() {
        let gh = stub(false, false);
        verify_provenance(&gh, Policy::Advisory, "dotfiles-linux-x86_64", b"data").unwrap();
    }

    #[test]
    fn required_policy_fails_when_gh_missing() {
        let gh = stub(false, false);
        let error = verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("gh CLI not found"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn required_policy_fails_when_attestation_missing() {
        let gh = stub(true, false);
        let error = verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no verified attestation"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn required_policy_succeeds_when_verified() {
        let gh = stub(true, true);
        verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data").unwrap();
    }

    #[test]
    fn advisory_policy_tolerates_gh_execution_failure() {
        let gh = StubGh {
            available: true,
            result: false,
            fails_to_run: true,
        };
        verify_provenance(&gh, Policy::Advisory, "dotfiles-linux-x86_64", b"data").unwrap();
    }

    #[test]
    fn staged_file_is_removed_after_verification() {
        // A dedicated asset name keeps this scan from seeing files staged by
        // sibling tests running on other threads.
        let asset = "cleanup-probe-x86_64";
        let gh = stub(true, true);
        verify_provenance(&gh, Policy::Advisory, asset, b"data").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .expect("temp dir should be readable")
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(asset))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary file should be cleaned up: {leftovers:?}"
        );
    }
}
