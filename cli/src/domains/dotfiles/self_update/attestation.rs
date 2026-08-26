//! GitHub build provenance verification for downloaded release assets.
//!
//! Release assets are attested by the release workflow with
//! `actions/attest-build-provenance`. After the SHA-256 checksum matches, the
//! downloaded bytes are additionally checked against that attestation using the
//! `gh` CLI.
//!
//! Verification is required by default. The policy can be relaxed explicitly
//! with one environment variable:
//!
//! - `DOTFILES_SKIP_ATTESTATION=1` — skip the check entirely.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::REPO;

/// Environment variable that disables provenance verification.
const SKIP_ENV: &str = "DOTFILES_SKIP_ATTESTATION";

/// How strictly provenance verification is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Policy {
    /// Do not verify provenance at all.
    Skip,
    /// Verification must succeed.
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verification {
    Verified,
    AuthenticationRequired,
    Unverified,
}

/// Resolve the policy from the supported flag value.
fn policy_from_flag(skip: Option<&str>) -> Policy {
    if skip == Some("1") {
        return Policy::Skip;
    }
    Policy::Required
}

/// Resolve the policy from the process environment.
pub(super) fn policy_from_env() -> Policy {
    let skip = std::env::var(SKIP_ENV).ok();
    policy_from_flag(skip.as_deref())
}

/// Abstraction over the `gh` CLI, enabling test injection.
pub(super) trait GhCli: std::fmt::Debug + Send + Sync {
    /// Whether the `gh` CLI is available on `PATH`.
    fn available(&self) -> bool;

    /// Verify the build provenance of the file at `path` for `repo`.
    ///
    /// Returns the verification outcome reported by `gh`.
    ///
    /// # Errors
    ///
    /// Returns an error only when `gh` cannot be executed at all.
    fn verify(&self, path: &Path, repo: &str) -> Result<Verification>;
}

/// Production [`GhCli`] backed by the `gh` executable on `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SystemGh;

impl GhCli for SystemGh {
    fn available(&self) -> bool {
        which::which("gh").is_ok()
    }

    fn verify(&self, path: &Path, repo: &str) -> Result<Verification> {
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
        Ok(classify_result(&result))
    }
}

fn classify_result(result: &crate::infra::exec::ExecResult) -> Verification {
    if result.success {
        Verification::Verified
    } else if result.code == Some(4) {
        Verification::AuthenticationRequired
    } else {
        Verification::Unverified
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
        return unverified(asset, "gh CLI not found; cannot verify build provenance");
    }

    let staged = stage_for_verification(asset, data)?;
    let verification = match gh.verify(staged.path(), REPO) {
        Ok(verification) => verification,
        Err(error) => {
            return unverified(asset, &format!("gh could not be executed: {error:#}"));
        }
    };

    match verification {
        Verification::Verified => {
            tracing::debug!("verified build provenance for {asset}");
            Ok(())
        }
        Verification::AuthenticationRequired => unverified(
            asset,
            "gh authentication required; run `gh auth login` or set GH_TOKEN/GITHUB_TOKEN and re-run",
        ),
        Verification::Unverified => unverified(asset, "gh reported no verified attestation"),
    }
}

/// Reject an asset whose provenance could not be verified.
fn unverified(asset: &str, reason: &str) -> Result<()> {
    bail!("build provenance verification failed for {asset}: {reason}");
}

/// Write `data` to a uniquely named temporary file for verification.
fn stage_for_verification(asset: &str, data: &[u8]) -> Result<crate::infra::fs::TempGuard> {
    let temp =
        crate::infra::fs::TempGuard::unique_file(&std::env::temp_dir(), ".dotfiles-attest", asset);
    std::fs::write(temp.path(), data).with_context(|| {
        format!(
            "writing {asset} to {} for provenance verification",
            temp.path().display()
        )
    })?;
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct StubGh {
        available: bool,
        result: Verification,
        fails_to_run: bool,
    }

    impl GhCli for StubGh {
        fn available(&self) -> bool {
            self.available
        }

        fn verify(&self, _path: &Path, _repo: &str) -> Result<Verification> {
            if self.fails_to_run {
                bail!("gh exploded");
            }
            Ok(self.result)
        }
    }

    const fn stub(available: bool, result: bool) -> StubGh {
        StubGh {
            available,
            result: if result {
                Verification::Verified
            } else {
                Verification::Unverified
            },
            fails_to_run: false,
        }
    }

    #[test]
    fn skip_flag_selects_skip_policy() {
        assert_eq!(policy_from_flag(Some("1")), Policy::Skip);
    }

    #[test]
    fn unset_or_disabled_skip_flag_selects_required_policy() {
        assert_eq!(policy_from_flag(None), Policy::Required);
        assert_eq!(policy_from_flag(Some("0")), Policy::Required);
    }

    #[test]
    fn skip_policy_does_not_invoke_gh() {
        let gh = stub(false, false);
        verify_provenance(&gh, Policy::Skip, "dotfiles-linux-x86_64", b"data").unwrap();
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
    fn required_policy_explains_when_gh_authentication_is_required() {
        let gh = StubGh {
            available: true,
            result: Verification::AuthenticationRequired,
            fails_to_run: false,
        };
        let error = verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("run `gh auth login`"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn gh_exit_code_four_is_classified_as_authentication_required() {
        let result = crate::infra::exec::ExecResult::failure(
            "",
            "To get started with GitHub CLI, please run: gh auth login",
            Some(4),
        );
        assert_eq!(
            classify_result(&result),
            Verification::AuthenticationRequired
        );
    }

    #[test]
    fn required_policy_succeeds_when_verified() {
        let gh = stub(true, true);
        verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data").unwrap();
    }

    #[test]
    fn required_policy_fails_on_gh_execution_failure() {
        let gh = StubGh {
            available: true,
            result: Verification::Unverified,
            fails_to_run: true,
        };
        let error = verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("gh could not be executed"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn staged_file_is_removed_after_verification() {
        // A dedicated asset name keeps this scan from seeing files staged by
        // sibling tests running on other threads.
        let asset = "cleanup-probe-x86_64";
        let gh = stub(true, true);
        verify_provenance(&gh, Policy::Required, asset, b"data").unwrap();
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
