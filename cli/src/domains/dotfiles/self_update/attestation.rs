//! GitHub build provenance verification for downloaded release assets.
//!
//! Release assets are attested by the release workflow with
//! `actions/attest-build-provenance`. After the SHA-256 checksum matches, the
//! downloaded bytes are additionally checked against that attestation using the
//! `gh` CLI.
//!
//! Verification is required by default. The policy can be relaxed explicitly
//! with a CLI flag or environment variable:
//!
//! - `--skip-attestation` — skip the check for this CLI invocation.
//! - `DOTFILES_SKIP_ATTESTATION=1` — skip the check entirely.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::REPO;

/// Number of verification attempts before an unverifiable asset is rejected.
const MAX_VERIFY_ATTEMPTS: u32 = 3;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verification {
    Verified,
    AuthenticationRequired,
    Unverified(String),
}

/// Resolve the policy from the CLI flag and environment variable.
fn policy_from_inputs(skip_flag: bool, skip_env: Option<&str>) -> Policy {
    if skip_flag || skip_env == Some("1") {
        return Policy::Skip;
    }
    Policy::Required
}

/// Resolve the policy for this invocation.
pub(super) fn policy(skip_flag: bool) -> Policy {
    let skip = std::env::var(SKIP_ENV).ok();
    policy_from_inputs(skip_flag, skip.as_deref())
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
        Ok(classify_result(&result))
    }
}

fn classify_result(result: &crate::infra::exec::ExecResult) -> Verification {
    if result.success {
        Verification::Verified
    } else if result.code == Some(4) {
        Verification::AuthenticationRequired
    } else {
        let output = if result.stderr.trim().is_empty() {
            result.stdout.trim()
        } else {
            result.stderr.trim()
        };
        let status = result
            .code
            .map_or_else(|| "no exit code".to_string(), |code| format!("exit {code}"));
        let detail = if output.is_empty() {
            format!("gh attestation verify failed with {status}")
        } else {
            format!("gh attestation verify failed with {status}: {output}")
        };
        Verification::Unverified(detail)
    }
}

/// Retry verification because `gh` depends on several remote GitHub and
/// Sigstore endpoints that can fail independently.
fn verify_with_retry(gh: &dyn GhCli, path: &Path, repo: &str) -> Result<Verification> {
    let mut attempt = 1_u32;
    loop {
        match gh.verify(path, repo) {
            Ok(Verification::Verified) => return Ok(Verification::Verified),
            Ok(Verification::AuthenticationRequired) => {
                return Ok(Verification::AuthenticationRequired);
            }
            Ok(Verification::Unverified(reason)) if attempt < MAX_VERIFY_ATTEMPTS => {
                tracing::debug!(
                    "attestation verification attempt {attempt}/{MAX_VERIFY_ATTEMPTS} failed; retrying: {reason}"
                );
            }
            Ok(verification) => return Ok(verification),
            Err(error) if attempt < MAX_VERIFY_ATTEMPTS => {
                tracing::debug!(
                    "attestation verification attempt {attempt}/{MAX_VERIFY_ATTEMPTS} could not run; retrying: {error:#}"
                );
            }
            Err(error) => return Err(error),
        }

        std::thread::sleep(retry_delay(attempt));
        attempt = attempt.saturating_add(1);
    }
}

#[cfg(not(test))]
fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(u64::from(attempt))
}

#[cfg(test)]
const fn retry_delay(_attempt: u32) -> std::time::Duration {
    std::time::Duration::ZERO
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
        tracing::debug!("provenance verification skipped by --skip-attestation or {SKIP_ENV}");
        return Ok(());
    }

    if !gh.available() {
        return unverified(asset, "gh CLI not found; cannot verify build provenance");
    }

    let staged = stage_for_verification(asset, data)?;
    let verification = match verify_with_retry(gh, staged.path(), REPO) {
        Ok(verification) => verification,
        Err(error) => {
            return unverified(
                asset,
                &format!(
                    "gh could not be executed after {MAX_VERIFY_ATTEMPTS} attempts: {error:#}"
                ),
            );
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
        Verification::Unverified(reason) => unverified(
            asset,
            &format!(
                "gh could not verify the attestation after {MAX_VERIFY_ATTEMPTS} attempts: {reason}"
            ),
        ),
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            Ok(self.result.clone())
        }
    }

    fn stub(available: bool, result: bool) -> StubGh {
        StubGh {
            available,
            result: if result {
                Verification::Verified
            } else {
                Verification::Unverified("gh reported no verified attestation".to_string())
            },
            fails_to_run: false,
        }
    }

    #[test]
    fn cli_flag_or_environment_selects_skip_policy() {
        assert_eq!(policy_from_inputs(true, None), Policy::Skip);
        assert_eq!(policy_from_inputs(false, Some("1")), Policy::Skip);
    }

    #[test]
    fn unset_or_disabled_bypasses_select_required_policy() {
        assert_eq!(policy_from_inputs(false, None), Policy::Required);
        assert_eq!(policy_from_inputs(false, Some("0")), Policy::Required);
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
    fn gh_failure_detail_is_preserved() {
        let result = crate::infra::exec::ExecResult::failure(
            "",
            "HTTP 503 from the attestation API",
            Some(1),
        );
        assert_eq!(
            classify_result(&result),
            Verification::Unverified(
                "gh attestation verify failed with exit 1: HTTP 503 from the attestation API"
                    .to_string()
            )
        );
    }

    #[derive(Debug)]
    struct FlakyGh {
        attempts: AtomicUsize,
    }

    impl GhCli for FlakyGh {
        fn available(&self) -> bool {
            true
        }

        fn verify(&self, _path: &Path, _repo: &str) -> Result<Verification> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                Ok(Verification::Unverified(
                    "temporary API failure".to_string(),
                ))
            } else {
                Ok(Verification::Verified)
            }
        }
    }

    #[test]
    fn transient_verification_failure_is_retried() {
        let gh = FlakyGh {
            attempts: AtomicUsize::new(0),
        };
        verify_provenance(&gh, Policy::Required, "dotfiles-linux-x86_64", b"data").unwrap();
        assert_eq!(gh.attempts.load(Ordering::SeqCst), 2);
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
            result: Verification::Unverified("not verified".to_string()),
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
