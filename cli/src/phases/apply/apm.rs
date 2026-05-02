//! Task: install AI plugin manifests via Microsoft APM.
//!
//! See <https://github.com/microsoft/apm>.  The dotfiles repo ships an
//! `apm/apm.yml` under `symlinks/` which is linked to `~/.apm/apm.yml` by the
//! [`InstallSymlinks`](super::symlinks::InstallSymlinks) task.  This task
//! shells out to `apm install -g --target copilot` so the manifest is consumed
//! at user scope and primitives deploy globally to the Copilot target
//! (`~/.copilot/`) rather than into this repository.  Idempotency is provided
//! by APM itself via its lockfile.

use anyhow::Result;

use crate::phases::{Context, Task, TaskPhase, TaskResult, task_deps};

/// Install AI plugin manifests via Microsoft APM (`apm install -g --target copilot`).
#[derive(Debug)]
pub struct InstallApmPackages;

impl Task for InstallApmPackages {
    fn name(&self) -> &'static str {
        "Install APM packages"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    task_deps![
        super::packages::InstallPackages,
        super::packages::InstallAurPackages,
        super::symlinks::InstallSymlinks
    ];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read()
            .root
            .join("symlinks")
            .join("apm")
            .join("apm.yml")
            .exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            return Ok(TaskResult::DryRun);
        }

        if !ctx.executor.which("apm") {
            let reason = "apm not found in PATH; install it via `winget install Microsoft.APM` \
                          on Windows or the `apm-bin` AUR package on Arch Linux"
                .to_string();
            ctx.log.warn(&format!("skipping: {reason}"));
            return Ok(TaskResult::Skipped(reason));
        }

        let cwd = ctx.home.clone();
        ctx.debug_fmt(|| {
            format!(
                "running `apm install -g --target copilot` in {} (interactive credential \
                 prompts disabled)",
                cwd.display()
            )
        });
        // Disable interactive credential prompts from git and Git Credential
        // Manager so a missing GitHub token causes apm install to fail fast
        // instead of blocking on a popup.
        let env: &[(&str, &str)] = &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GCM_INTERACTIVE", "Never"),
            ("GCM_GUI_PROMPT", "false"),
        ];
        match ctx.executor.run_in_with_env(
            &cwd,
            "apm",
            &["install", "-g", "--target", "copilot"],
            env,
        ) {
            Ok(_) => Ok(TaskResult::Ok),
            Err(err) => {
                let msg = format!("{err:#}");
                if looks_like_auth_failure(&msg) {
                    let reason = "apm install requires GitHub authentication; run \
                                  `gh auth login` or set GH_TOKEN/GITHUB_TOKEN and re-run"
                        .to_string();
                    ctx.log
                        .warn(&format!("skipping: {reason} (details: {})", msg.trim()));
                    Ok(TaskResult::Skipped(reason))
                } else {
                    Err(err)
                }
            }
        }
    }
}

/// Heuristic: does an `apm install` failure message indicate a missing or
/// invalid GitHub credential rather than a real installation error?
fn looks_like_auth_failure(message: &str) -> bool {
    let lowered = message.to_lowercase();
    [
        "authentication",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "401",
        "403",
        "fatal: authentication failed",
        "credential",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::phases::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_when_no_apm_yml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(!InstallApmPackages.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_apm_yml_exists() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let apm_dir = dir.path().join("symlinks").join("apm");
        std::fs::create_dir_all(&apm_dir).expect("create symlinks/apm dir");
        std::fs::write(apm_dir.join("apm.yml"), "name: test\n").expect("write apm.yml");
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(InstallApmPackages.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_apm_not_found() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let result = InstallApmPackages.run(&ctx).expect("run should not error");
        match result {
            TaskResult::Skipped(reason) => assert!(
                reason.contains("apm not found"),
                "expected reason to mention 'apm not found', got {reason:?}"
            ),
            other => panic!("expected TaskResult::Skipped, got {other:?}"),
        }
    }
}
