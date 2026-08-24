//! APM update task: delegate locked dependency advancement to APM.

use anyhow::Result;

use super::ApmFragmentSource;
use super::commands::{ApmCommand, ApmCommandResult, run_apm_invocation};
use super::install::apm_task_should_run;
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use super::manifest::read_lock_snapshot;
use super::skip;
use super::targets::missing_apm_reason;
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

/// Advance pinned APM dependency versions under the `update` command.
///
/// The catalog dependency on [`super::InstallApmPackages`] ensures the generated
/// manifest is converged before APM advances matching refs.
#[derive(Debug)]
pub struct UpdateApmPackages {
    fragments: ConfigHandle<Vec<ApmFragmentSource>>,
}

impl UpdateApmPackages {
    /// Create the update task with the managed symlink configuration that
    /// supplies APM fragments.
    #[must_use]
    pub const fn new(fragments: ConfigHandle<Vec<ApmFragmentSource>>) -> Self {
        Self { fragments }
    }
}

impl Task for UpdateApmPackages {
    task_metadata! {
        name: "APM package updates",
        selector: "apm-update",
        update_only: true,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx, &self.fragments.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.system().which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let targets = ManagedTargets::detect(ctx)?;
        if ctx.dry_run() {
            return preview_apm_update(ctx, targets);
        }
        advance_apm_dependencies(ctx, targets)
    }
}

fn preview_apm_update(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
    match run_apm_invocation(ctx, ApmCommand::Update, &["update", "-g", "--dry-run"])? {
        ApmCommandResult::Success => {
            ctx.log().dry_run(
                "use APM's update plan to advance dependencies to their latest matching refs",
            );
            targets.preview(ctx, ManagedTargetPreview::Update);
            Ok(TaskResult::DryRun)
        }
        ApmCommandResult::AuthSkipped(reason) => Ok(TaskResult::unmet(reason)),
    }
}

fn advance_apm_dependencies(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
    let lock_path = ctx.home().join(".apm").join("apm.lock.yaml");
    let lock_before = read_lock_snapshot(&lock_path)?;
    let target_snapshot = targets.snapshot(ctx);

    match targets.run_apm_command(ctx, ApmCommand::Update)? {
        ApmCommandResult::AuthSkipped(reason) => Ok(TaskResult::unmet(reason)),
        ApmCommandResult::Success => {
            let lock_after = read_lock_snapshot(&lock_path)?;
            let autopilot_changed = targets.finish(ctx, &target_snapshot);
            if lock_before != lock_after || autopilot_changed {
                ctx.log().trace("updated: advanced to latest versions");
                Ok(
                    TaskStats::changed_with_message("advanced APM dependencies to latest versions")
                        .finish(),
                )
            } else {
                ctx.log().debug("APM dependencies already at latest refs");
                Ok(TaskResult::Ok)
            }
        }
    }
}
