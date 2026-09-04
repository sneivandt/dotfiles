//! APM install task: merge fragments, write the generated manifest, and
//! delegate user-scope convergence to APM.

use std::path::PathBuf;

use anyhow::Result;

use super::ApmFragmentSource;
use super::commands::{ApmCommand, install_task_result};
use super::fragments::{discover_effective_fragment_files, merge_fragments};
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use super::manifest::{
    describe_lock_changes, merged_manifest_needs_write, read_lock_snapshot, write_merged_manifest,
};
use super::skip;
use super::targets::missing_apm_reason;
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

/// Converge AI plugin manifests via Microsoft APM.
///
/// The generated manifest remains dotfiles-owned because public and private
/// fragments must be merged. APM owns dependency resolution, local-source
/// integrity, deployment convergence, and stale/orphan cleanup.
#[derive(Debug)]
pub struct InstallApmPackages {
    fragments: ConfigHandle<Vec<ApmFragmentSource>>,
}

impl InstallApmPackages {
    /// Create the task with the managed symlink configuration that supplies APM
    /// fragments.
    #[must_use]
    pub const fn new(fragments: ConfigHandle<Vec<ApmFragmentSource>>) -> Self {
        Self { fragments }
    }
}

impl Task for InstallApmPackages {
    task_metadata! {
        name: "APM packages",
        selector: "apm",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx, &self.fragments.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.dry_run() && !ctx.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let fragments = discover_effective_fragment_files(ctx.home(), &self.fragments.read())?;
        if fragments.is_empty() {
            return Ok(skip("no manifest fragments found under ~/.apm/config/"));
        }

        let plan = ApmInstallPlan::build(ctx, &fragments)?;
        if ctx.dry_run() {
            return Ok(plan.preview(ctx));
        }
        plan.apply(ctx)
    }
}

#[derive(Debug)]
struct ApmInstallPlan {
    targets: ManagedTargets,
    fragment_count: usize,
    manifest_path: PathBuf,
    lock_path: PathBuf,
    merged: String,
    manifest_needs_write: bool,
}

impl ApmInstallPlan {
    fn build(ctx: &Context, fragments: &[PathBuf]) -> Result<Self> {
        let apm_dir = ctx.home().join(".apm");
        let manifest_path = apm_dir.join("apm.yml");
        let lock_path = apm_dir.join("apm.lock.yaml");
        let merged = merge_fragments(fragments)?;
        let manifest_needs_write = merged_manifest_needs_write(&manifest_path, &merged)?;
        Ok(Self {
            targets: ManagedTargets::detect(ctx)?,
            fragment_count: fragments.len(),
            manifest_path,
            lock_path,
            merged,
            manifest_needs_write,
        })
    }

    fn preview(&self, ctx: &Context) -> TaskResult {
        let mut planned = 1_u32;
        if self.manifest_needs_write {
            ctx.log().dry_run(format!(
                "merge {} APM manifest fragment(s) into {}",
                self.fragment_count,
                self.manifest_path.display()
            ));
            planned = planned.saturating_add(1);
        }
        ctx.log().dry_run(
            "run apm install -g to converge dependencies and remove stale user-scope deployments",
        );
        planned = planned.saturating_add(self.targets.preview(ctx, ManagedTargetPreview::Install));
        TaskStats::from_counts(planned, 0, 0, 0).finish()
    }

    fn apply(&self, ctx: &Context) -> Result<TaskResult> {
        let lock_before = read_lock_snapshot(&self.lock_path)?;
        let target_snapshot = self.targets.snapshot(ctx);

        if self.manifest_needs_write {
            write_merged_manifest(&self.manifest_path, &self.merged)?;
        }

        let install_result =
            install_task_result(self.targets.run_apm_command(ctx, ApmCommand::Install)?);
        if !matches!(install_result, TaskResult::Ok) {
            return Ok(install_result);
        }

        let lock_after = read_lock_snapshot(&self.lock_path)?;
        let autopilot_changed = self.targets.finish(ctx, &target_snapshot);
        let lock_changed = lock_before != lock_after;
        let dependency_changes =
            describe_lock_changes(lock_before.as_deref(), lock_after.as_deref());
        for detail in &dependency_changes {
            ctx.log().info(detail);
        }
        if self.manifest_needs_write {
            ctx.log().info("updated: generated APM manifest");
        }
        if lock_changed && dependency_changes.is_empty() {
            ctx.log().info("updated: APM lock state");
        }

        let changed = self.manifest_needs_write || lock_changed || autopilot_changed;
        if changed {
            let message = if dependency_changes.is_empty() {
                "updated APM configuration".to_string()
            } else {
                changed_dependency_summary(dependency_changes.len())
            };
            ctx.log().trace(format!("APM change summary: {message}"));
            Ok(TaskStats::changed_with_message(message).finish())
        } else {
            ctx.log()
                .debug("APM dependencies and deployments already current");
            Ok(TaskResult::Ok)
        }
    }
}

fn changed_dependency_summary(count: usize) -> String {
    if count == 1 {
        "changed 1 APM dependency".to_string()
    } else {
        format!("changed {count} APM dependencies")
    }
}

/// Whether an APM task should run on this machine.
pub(super) fn apm_task_should_run(ctx: &Context, fragments: &[ApmFragmentSource]) -> bool {
    match discover_effective_fragment_files(ctx.home(), fragments) {
        Ok(fragments) => !fragments.is_empty(),
        Err(err) => {
            ctx.log().warn(format!(
                "could not inspect APM fragments; task will run: {err:#}"
            ));
            true
        }
    }
}
