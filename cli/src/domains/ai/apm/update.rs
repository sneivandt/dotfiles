//! APM update task: advance locked dependency refs for the `update` command.

use std::path::Path;

use anyhow::{Context as _, Result};

use super::autopilot::{apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids};
use super::commands::{
    APM_NONINTERACTIVE_ENV, ApmCommand, ApmCommandResult, report_apm_output, run_apm_command,
};
use super::fragments::{discover_fragment_files, merge_fragments};
use super::install::apm_task_should_run;
use super::manifest::{manifest_fingerprint, manifest_marker_matches};
use super::outdated::{ApmOutdatedCheck, ApmUpdateOutcome, outdated_output_has_updates};
use super::skip_with_warning;
use super::targets::{ApmTargets, missing_apm_reason};
use crate::engine::{Context, Task, TaskPhase, TaskResult, task_metadata};

/// Advance pinned APM dependency versions — the `update` command only.
///
/// This task runs in [`TaskPhase::Update`], which the scheduler executes after
/// the Provision phase (where [`super::InstallApmPackages`] has already
/// converged the manifest and lockfile).  It is only scheduled by the `update`
/// command, so ordinary installs never run version-advancing tasks.
///
/// Because phases run independently — a failed Provision task does not abort the
/// run before the Update phase — this task re-asserts the convergence
/// precondition itself: it only contacts APM when a current manifest has been
/// installed successfully (lockfile present and the success marker matches the
/// merged manifest hash).  Otherwise it skips, so a half-converged or failed
/// install can never trigger a lockfile-advancing `apm update`.
#[derive(Debug)]
pub struct UpdateApmPackages;

impl Task for UpdateApmPackages {
    task_metadata! {
        name: "Update APM packages",
        phase: TaskPhase::Update,
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
        // restores the "advance only after a successful install" invariant that
        // the single-task design enforced via early return, now that
        // convergence runs in a separate, independently-failing phase.
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
            return preview_apm_update(ctx, targets);
        }
        advance_apm_dependencies(ctx, targets)
    }
}

fn preview_apm_update(ctx: &Context, targets: ApmTargets) -> Result<TaskResult> {
    Ok(match apm_dependencies_are_outdated(ctx)? {
        ApmOutdatedCheck::Outdated(true) => {
            ctx.log().dry_run(
                "run apm update -g --yes with auto-detected runtimes to advance stale dependencies",
            );
            if targets.includes_copilot_app() {
                ctx.log().dry_run(
                    "run apm install -g --target copilot-app to redeploy updated Copilot App \
                     workflows separately, then re-assert them to autopilot + enabled in \
                     ~/.copilot/data.db",
                );
            }
            TaskResult::DryRun
        }
        ApmOutdatedCheck::Outdated(false) => {
            ctx.log().debug("APM dependencies are up-to-date");
            TaskResult::Ok
        }
        ApmOutdatedCheck::Skipped(reason) => TaskResult::Skipped(reason),
    })
}

/// Advance locked user-scope dependencies to the latest matching refs.
///
/// Runs only under the `update` command. Checks `apm outdated -g` first and
/// only runs `apm update --yes` when something is stale. Returns the resulting
/// task outcome: `Ok` when nothing needed advancing or an update succeeded, or
/// `Skipped` when credentials are unavailable.
fn advance_apm_dependencies(ctx: &Context, targets: ApmTargets) -> Result<TaskResult> {
    Ok(match apm_dependencies_are_outdated(ctx)? {
        ApmOutdatedCheck::Outdated(true) => {
            // Snapshot which dotfiles-managed workflows are already armed before
            // the follow-up Copilot App install redeploys workflows
            // secure-by-default after the unscoped dependency update.
            let pre_workflows = targets
                .includes_copilot_app()
                .then(|| snapshot_desired_apm_workflow_ids(ctx));
            match run_apm_update(ctx, targets)? {
                ApmUpdateOutcome::Changed => {
                    ctx.log().always("    updated: advanced to latest versions");
                    if let Some(pre) = pre_workflows {
                        apply_workflow_autopilot_fixup(ctx, &pre);
                    }
                    TaskResult::OkWithMessage("advanced APM dependencies to latest versions".into())
                }
                ApmUpdateOutcome::Unchanged => {
                    ctx.log().debug("APM dependencies already at latest refs");
                    if let Some(pre) = pre_workflows {
                        apply_workflow_autopilot_fixup(ctx, &pre);
                    }
                    TaskResult::Ok
                }
                ApmUpdateOutcome::Skipped(reason) => TaskResult::Skipped(reason),
            }
        }
        ApmOutdatedCheck::Outdated(false) => {
            ctx.log().debug("APM dependencies are up-to-date");
            TaskResult::Ok
        }
        ApmOutdatedCheck::Skipped(reason) => TaskResult::Skipped(reason),
    })
}

/// Return whether any locked user-scope dependency has a newer matching ref.
fn apm_dependencies_are_outdated(ctx: &Context) -> Result<ApmOutdatedCheck> {
    let system = ctx.system();
    let cwd = system.home();
    ctx.debug_fmt(|| format!("running `apm outdated -g` in {}", cwd.display()));
    match system
        .executor()
        .run_in_with_env(cwd, "apm", &["outdated", "-g"], APM_NONINTERACTIVE_ENV)
    {
        Ok(result) => {
            report_apm_output(ctx, &result.stdout, &result.stderr);
            Ok(ApmOutdatedCheck::Outdated(outdated_output_has_updates(
                &result.stdout,
                &result.stderr,
            )))
        }
        Err(err) => {
            let msg = format!("{err:#}");
            if super::commands::looks_like_auth_failure(&msg) {
                let reason = "apm outdated requires GitHub authentication; run \
                              `gh auth login` or set GH_TOKEN/GITHUB_TOKEN and re-run"
                    .to_string();
                ctx.log().warn(&format!(
                    "skipping APM update check: {reason} (details: {})",
                    msg.trim()
                ));
                Ok(ApmOutdatedCheck::Skipped(reason))
            } else {
                Err(err).context("checking for outdated APM dependencies")
            }
        }
    }
}

/// Refresh locked user-scope dependencies to the latest matching refs.
///
/// Detects whether anything actually advanced by snapshotting the APM lockfile
/// (`~/.apm/apm.lock.yaml`) before and after the run rather than parsing console
/// output.  APM emits no stable "no changes" marker for dependencies pinned to
/// git branch/commit refs — `apm outdated` reports them as `unknown`, so APM
/// re-integrates them on every run even when no ref moves.  The lockfile only
/// changes when a pinned ref actually advances, making it the authoritative
/// change signal.
fn run_apm_update(ctx: &Context, targets: ApmTargets) -> Result<ApmUpdateOutcome> {
    let lock_path = ctx.home().join(".apm").join("apm.lock.yaml");
    let lock_before = read_lock_snapshot(&lock_path);
    match run_apm_command(ctx, ApmCommand::Update, targets)? {
        ApmCommandResult::Success | ApmCommandResult::ToleratedWorkflowEncodeFailures => {
            let lock_after = read_lock_snapshot(&lock_path);
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
/// A missing lockfile (or any read error) degrades to `None` so update
/// detection never fails the task; an unreadable lockfile that hashes the same
/// way both times simply reports no change.
fn read_lock_snapshot(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}
