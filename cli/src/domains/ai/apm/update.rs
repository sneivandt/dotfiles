//! APM update task: advance locked dependency refs for the `update` command.

use std::path::Path;

use anyhow::{Context as _, Result};

use super::autopilot::{apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids};
use super::commands::{ApmCommand, ApmCommandResult, run_apm_command};
use super::fragments::{discover_fragment_files, merge_fragments};
use super::install::apm_task_should_run;
use super::manifest::{manifest_fingerprint, manifest_marker_matches};
use super::skip_with_warning;
use super::targets::{ApmTargets, missing_apm_reason};
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};

enum ApmUpdateOutcome {
    Changed,
    Unchanged,
    Skipped(String),
}

/// Advance pinned APM dependency versions — the `update` command only.
///
/// This task is only scheduled by the `update` command. Its catalog dependency
/// on [`super::InstallApmPackages`] ensures manifest convergence completes
/// before dependency advancement.
///
/// The task also re-asserts the convergence precondition before contacting APM:
/// the lockfile must exist and the success marker must match the merged manifest
/// hash. A half-converged install can therefore never advance the lockfile.
#[derive(Debug)]
pub struct UpdateApmPackages;

impl Task for UpdateApmPackages {
    task_metadata! {
        name: "Update APM packages",
        update_only: true,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let system = ctx.system();
        let home = system.home();
        if !system.which("apm") {
            return Ok(skip_with_warning(ctx, missing_apm_reason(ctx)));
        }

        let fragments = discover_fragment_files(home)?;
        if fragments.is_empty() {
            return Ok(skip_with_warning(
                ctx,
                "no manifest fragments found under ~/.apm/config/",
            ));
        }

        // Re-assert the convergence precondition: only advance locked refs when
        // the current merged manifest has been installed successfully.  This
        // preserves the "advance only after a successful install" invariant.
        let lock_path = home.join(".apm").join("apm.lock.yaml");
        let lock_present = lock_path
            .try_exists()
            .with_context(|| format!("checking APM lockfile {}", lock_path.display()))?;
        let merged = merge_fragments(&fragments)?;
        let manifest_hash = manifest_fingerprint(&merged);
        let marker_path = home.join(".apm").join(".dotfiles-manifest.sha256");
        let marker_matches = manifest_marker_matches(&marker_path, &manifest_hash)?;
        if !lock_present || !marker_matches {
            let reason = "APM manifest has not been installed successfully yet; skipping \
                          dependency advancement"
                .to_string();
            ctx.log().debug(&reason);
            return Ok(TaskResult::Skipped(reason));
        }

        let targets = ApmTargets::detect(ctx)?;
        if ctx.dry_run() {
            return Ok(preview_apm_update(ctx, targets));
        }
        advance_apm_dependencies(ctx, targets)
    }
}

fn preview_apm_update(ctx: &Context, targets: ApmTargets) -> TaskResult {
    ctx.log().dry_run(
        "run apm update -g --yes with auto-detected runtimes; APM skips dependencies already at \
         their latest matching refs",
    );
    if targets.includes_copilot_app() {
        ctx.log().dry_run(
            "run apm install -g --target copilot-app to redeploy updated Copilot App workflows \
             separately, then re-assert them to autopilot + enabled in ~/.copilot/data.db",
        );
    }
    TaskStats::changed().finish()
}

/// Advance locked user-scope dependencies to the latest matching refs.
///
/// Runs only under the `update` command. APM's update command is idempotent, so
/// it runs directly and the lockfile determines whether any ref advanced.
fn advance_apm_dependencies(ctx: &Context, targets: ApmTargets) -> Result<TaskResult> {
    let pre_workflows = targets
        .includes_copilot_app()
        .then(|| snapshot_desired_apm_workflow_ids(ctx));
    let result = match run_apm_update(ctx, targets)? {
        ApmUpdateOutcome::Changed => {
            ctx.log().always("    updated: advanced to latest versions");
            TaskStats::changed_with_message("advanced APM dependencies to latest versions").finish()
        }
        ApmUpdateOutcome::Unchanged => {
            ctx.log().debug("APM dependencies already at latest refs");
            TaskResult::Ok
        }
        ApmUpdateOutcome::Skipped(reason) => return Ok(TaskResult::Skipped(reason)),
    };
    if let Some(pre) = pre_workflows {
        apply_workflow_autopilot_fixup(ctx, &pre);
    }
    Ok(result)
}

/// Refresh locked user-scope dependencies to the latest matching refs.
///
/// Detects whether anything actually advanced by snapshotting the APM lockfile
/// (`~/.apm/apm.lock.yaml`) before and after the run rather than parsing console
/// output. The lockfile only changes when a pinned ref actually advances,
/// making it the authoritative change signal.
fn run_apm_update(ctx: &Context, targets: ApmTargets) -> Result<ApmUpdateOutcome> {
    let lock_path = ctx.home().join(".apm").join("apm.lock.yaml");
    let lock_before = read_lock_snapshot(&lock_path)?;
    match run_apm_command(ctx, ApmCommand::Update, targets)? {
        ApmCommandResult::Success | ApmCommandResult::ToleratedWorkflowEncodeFailures => {
            let lock_after = read_lock_snapshot(&lock_path)?;
            if lock_before == lock_after {
                Ok(ApmUpdateOutcome::Unchanged)
            } else {
                Ok(ApmUpdateOutcome::Changed)
            }
        }
        ApmCommandResult::AuthSkipped(reason) => Ok(ApmUpdateOutcome::Skipped(reason)),
    }
}

/// Read the APM lockfile for before/after change detection.
///
/// A missing lockfile is represented as `None`; other errors are surfaced.
fn read_lock_snapshot(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading APM lockfile {}", path.display())),
    }
}
