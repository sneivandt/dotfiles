//! APM install task: merge fragments, write the generated manifest, and
//! delegate user-scope convergence to APM.

use std::path::PathBuf;

use anyhow::Result;

use super::ApmFragmentSource;
use super::commands::{ApmCommand, ApmCommandResult};
use super::fragments::{discover_effective_fragment_files, merge_fragments};
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use super::manifest::{
    describe_lock_changes, merged_manifest_needs_write, read_lock_snapshot, write_merged_manifest,
};
use super::skip;
use super::targets::missing_apm_reason;
use super::update::preview_apm_update;
use crate::engine::{Context, Task, TaskMeta, TaskResult, TaskStats};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

/// Select the native APM operation for an install command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApmPackageMode {
    /// Converge the locked dependency graph without advancing refs.
    Install,
    /// Advance eligible refs and converge the resulting dependency graph.
    UpdatePins,
}

impl ApmPackageMode {
    const fn command(self) -> ApmCommand {
        match self {
            Self::Install => ApmCommand::Install,
            Self::UpdatePins => ApmCommand::Update,
        }
    }
}

/// Converge AI plugin manifests via Microsoft APM.
///
/// The generated manifest remains dotfiles-owned because public and private
/// fragments must be merged. APM owns dependency resolution, local-source
/// integrity, deployment convergence, and stale/orphan cleanup.
#[derive(Debug)]
pub struct InstallApmPackages {
    fragments: ConfigHandle<Vec<ApmFragmentSource>>,
    mode: ApmPackageMode,
}

impl InstallApmPackages {
    /// Create the task with the managed symlink configuration that supplies APM
    /// fragments.
    #[must_use]
    pub const fn new(
        fragments: ConfigHandle<Vec<ApmFragmentSource>>,
        mode: ApmPackageMode,
    ) -> Self {
        Self { fragments, mode }
    }
}

impl Task for InstallApmPackages {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("APM packages")
            .with_selector("apm")
            .with_update_only(self.mode == ApmPackageMode::UpdatePins)
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx, &self.fragments.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if (!ctx.dry_run() || self.mode == ApmPackageMode::UpdatePins) && !ctx.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let fragments = discover_effective_fragment_files(ctx.home(), &self.fragments.read())?;
        if fragments.is_empty() {
            return Ok(skip("no manifest fragments found under ~/.apm/config/"));
        }

        let plan = ApmInstallPlan::build(ctx, &fragments, self.mode)?;
        if ctx.dry_run() {
            return plan.preview(ctx);
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
    mode: ApmPackageMode,
}

impl ApmInstallPlan {
    fn build(ctx: &Context, fragments: &[PathBuf], mode: ApmPackageMode) -> Result<Self> {
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
            mode,
        })
    }

    fn preview(&self, ctx: &Context) -> Result<TaskResult> {
        if self.mode == ApmPackageMode::UpdatePins && !self.manifest_needs_write {
            return preview_apm_update(ctx, self.targets);
        }

        let mut planned = 1_u32;
        if self.manifest_needs_write {
            ctx.log().dry_run(format!(
                "merge {} APM manifest fragment(s) into {}",
                self.fragment_count,
                self.manifest_path.display()
            ));
            planned = planned.saturating_add(1);
        }
        let preview = match self.mode {
            ApmPackageMode::Install => ManagedTargetPreview::Install,
            ApmPackageMode::UpdatePins => ManagedTargetPreview::Update,
        };
        match self.mode {
            ApmPackageMode::Install => ctx.log().dry_run(
                "run apm install -g to converge dependencies and remove stale user-scope deployments",
            ),
            ApmPackageMode::UpdatePins => ctx.log().dry_run(
                "run apm update -g --yes to advance matching refs and converge deployments",
            ),
        }
        planned = planned.saturating_add(self.targets.preview(ctx, preview));
        Ok(TaskStats::from_counts(planned, 0, 0, 0).finish())
    }

    fn apply(&self, ctx: &Context) -> Result<TaskResult> {
        let lock_before = read_lock_snapshot(&self.lock_path)?;
        let target_snapshot = self.targets.snapshot(ctx);

        if self.manifest_needs_write {
            write_merged_manifest(&self.manifest_path, &self.merged)?;
        }

        let command = self.targets.run_apm_command(ctx, self.mode.command());
        // A later target can fail after APM reset Copilot App workflows.
        // Restore retained policy before propagating any convergence failure.
        let autopilot_changed = self.targets.finish(ctx, &target_snapshot);
        let command = command?;
        if let ApmCommandResult::AuthSkipped(reason) = command.outcome {
            return Ok(TaskResult::unmet(reason));
        }

        let lock_after = read_lock_snapshot(&self.lock_path)?;
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

        let changed =
            self.manifest_needs_write || lock_changed || autopilot_changed || command.changed;
        if changed {
            let message = match (self.mode, dependency_changes.is_empty()) {
                (_, true) if self.manifest_needs_write => "updated APM configuration".to_string(),
                (ApmPackageMode::Install, true) => "updated APM configuration".to_string(),
                (ApmPackageMode::UpdatePins, true) => "updated APM deployments".to_string(),
                (ApmPackageMode::Install, false) => {
                    changed_dependency_summary(dependency_changes.len())
                }
                (ApmPackageMode::UpdatePins, false) => {
                    updated_dependency_summary(dependency_changes.len())
                }
            };
            let summary = match self.mode {
                ApmPackageMode::Install => "APM change summary",
                ApmPackageMode::UpdatePins => "APM update summary",
            };
            ctx.log().trace(format!("{summary}: {message}"));
            Ok(TaskStats::changed_with_message(message).finish())
        } else {
            let message = match self.mode {
                ApmPackageMode::Install => "APM dependencies and deployments already current",
                ApmPackageMode::UpdatePins => "APM dependencies already at latest refs",
            };
            ctx.log().debug(message);
            Ok(TaskResult::Ok)
        }
    }
}

fn updated_dependency_summary(count: usize) -> String {
    if count == 1 {
        "updated 1 APM dependency".to_string()
    } else {
        format!("updated {count} APM dependencies")
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
