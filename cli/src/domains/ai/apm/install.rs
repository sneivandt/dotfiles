//! APM install task: merge fragments, write the generated manifest, and run
//! `apm install` without advancing dependency refs.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::ApmFragmentSource;
use super::autopilot::{apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids};
use super::commands::{
    ApmCommand, ensure_copilot_app_enabled, install_task_result, prune_user_scope, run_apm_command,
};
use super::fragments::{discover_effective_fragment_files, merge_fragments};
use super::manifest::{
    describe_dependencies, manifest_marker_matches, merged_manifest_needs_write,
    write_manifest_marker, write_merged_manifest,
};
use super::skip;
use super::sources::install_fingerprint;
use super::targets::{ApmTargets, missing_apm_reason};
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

/// Converge AI plugin manifests via Microsoft APM.
///
/// Merges the manifest fragments and runs `apm install` when the manifest,
/// local plugin sources, or resolved targets have changed since the last
/// successful install.  It never advances locked dependency refs — that is
/// [`super::update::UpdateApmPackages`]'s job under the `update` command.
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
        let system = ctx.system();
        let home = system.home();
        if !ctx.dry_run() && !system.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let fragments = discover_effective_fragment_files(home, &self.fragments.read())?;
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

/// Immutable description of the work required to converge APM.
///
/// Both preview and apply consume this same filesystem-derived decision so
/// dry-run cannot drift from real execution.
#[derive(Debug)]
struct ApmInstallPlan {
    change: ApmInstallChange,
    targets: ApmTargets,
    fragment_count: usize,
    manifest_path: PathBuf,
    lock_path: PathBuf,
    marker_path: PathBuf,
    merged: String,
    manifest_hash: String,
}

impl ApmInstallPlan {
    /// Build an install plan from the effective manifest fragments and current
    /// APM artifacts.
    fn build(ctx: &Context, fragments: &[PathBuf]) -> Result<Self> {
        let home = ctx.home();
        let apm_dir = home.join(".apm");
        let manifest_path = apm_dir.join("apm.yml");
        let lock_path = apm_dir.join("apm.lock.yaml");
        let marker_path = apm_dir.join(".dotfiles-manifest.sha256");
        let merged = merge_fragments(fragments)?;
        let targets = ApmTargets::detect(ctx)?;
        let manifest_hash = install_fingerprint(&merged, home, targets.includes_copilot_app())?;
        let change = ApmInstallChange::detect(
            &manifest_path,
            &lock_path,
            &marker_path,
            &merged,
            &manifest_hash,
        )?;
        Ok(Self {
            change,
            targets,
            fragment_count: fragments.len(),
            manifest_path,
            lock_path,
            marker_path,
            merged,
            manifest_hash,
        })
    }

    /// Report the plan without mutating APM state.
    fn preview(&self, ctx: &Context) -> TaskResult {
        let mut planned = match self.change {
            ApmInstallChange::Current => return TaskResult::Ok,
            ApmInstallChange::ManifestChanged => {
                ctx.log().dry_run(format!(
                    "merge {} APM manifest fragment(s) into {}",
                    self.fragment_count,
                    self.manifest_path.display()
                ));
                ctx.log().dry_run(
                    "run apm install -g with manifest-resolved runtimes to sync changed manifest",
                );
                2_u32
            }
            ApmInstallChange::LockMissing => {
                ctx.log().dry_run(format!(
                    "run apm install -g with manifest-resolved runtimes because {} is missing",
                    self.lock_path.display()
                ));
                1
            }
            ApmInstallChange::MarkerStale => {
                ctx.log().dry_run(
                    "run apm install -g with manifest-resolved runtimes because the current \
                     manifest has not been installed successfully yet",
                );
                1
            }
        };
        if self.targets.includes_copilot_app() {
            ctx.log().dry_run(
                "run apm install -g --target copilot-app to sync Copilot App workflows separately",
            );
            planned = planned.saturating_add(1);
        }
        TaskStats::from_counts(planned, 0, 0, 0).finish()
    }

    /// Apply the planned APM convergence.
    fn apply(&self, ctx: &Context) -> Result<TaskResult> {
        // `manifest_hash` covers the merged manifest, the content of every
        // locally symlinked plugin source, and the resolved target set, so a
        // matching marker means the last successful install already deployed
        // exactly this.  Re-running `apm install` would spawn several seconds
        // of subprocesses to reach the state we are already in.
        if self.change == ApmInstallChange::Current {
            ctx.debug_fmt(|| {
                "APM manifest, local plugin sources, and targets are unchanged since the last \
                 successful install; skipping apm install"
                    .to_string()
            });
            return Ok(TaskResult::Ok);
        }

        let pre_workflows = self
            .targets
            .includes_copilot_app()
            .then(|| snapshot_desired_apm_workflow_ids(ctx));
        if self.targets.includes_copilot_app() {
            ensure_copilot_app_enabled(ctx);
        }

        if self.change == ApmInstallChange::ManifestChanged {
            write_merged_manifest(&self.manifest_path, &self.merged)?;
        }

        // Reached only when something APM cares about actually changed: a
        // manifest edit, a local plugin edit, a new target, a missing lockfile,
        // or a marker that never recorded a successful install.
        let install_result =
            install_task_result(run_apm_command(ctx, ApmCommand::Install, self.targets)?);
        if !matches!(install_result, TaskResult::Ok) {
            // Auth skip (or similar): do not record the manifest as installed
            // and do not attempt to advance dependencies.
            return Ok(install_result);
        }
        // Run log only: the task's own status row already reports the same
        // phrase as its reason, so surfacing it as a detail line would state
        // the change twice.
        ctx.log().trace(format!(
            "installed: {}",
            describe_dependencies(&self.merged)
        ));
        write_manifest_marker(&self.marker_path, &self.manifest_hash)?;
        prune_user_scope(ctx)?;

        // Convergence is complete.  Advancing locked dependency refs
        // is a separate concern handled by the `update`-only task, so this task
        // never moves a locked ref forward.
        if let Some(pre) = pre_workflows {
            apply_workflow_autopilot_fixup(ctx, &pre);
        }
        Ok(TaskStats::changed_with_message(format!(
            "installed {}",
            describe_dependencies(&self.merged)
        ))
        .finish())
    }
}

/// The single reason an APM install is either current or needs convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApmInstallChange {
    /// Manifest, lockfile, and success marker all describe the desired state.
    Current,
    /// The merged manifest differs from `~/.apm/apm.yml`.
    ManifestChanged,
    /// The APM lockfile is absent.
    LockMissing,
    /// The success marker is missing or does not match the desired fingerprint.
    MarkerStale,
}

impl ApmInstallChange {
    /// Detect the highest-priority reason APM needs to converge.
    ///
    /// # Errors
    ///
    /// Propagates IO errors from comparing the merged manifest against the
    /// target, probing the lockfile, or reading the success marker.
    fn detect(
        manifest_path: &Path,
        lock_path: &Path,
        marker_path: &Path,
        merged: &str,
        manifest_hash: &str,
    ) -> Result<Self> {
        let manifest_needs_write = merged_manifest_needs_write(manifest_path, merged)?;
        let lock_missing = !lock_path
            .try_exists()
            .with_context(|| format!("checking APM lockfile {}", lock_path.display()))?;
        let marker_missing_or_stale = !manifest_marker_matches(marker_path, manifest_hash)?;
        Ok(if manifest_needs_write {
            Self::ManifestChanged
        } else if lock_missing {
            Self::LockMissing
        } else if marker_missing_or_stale {
            Self::MarkerStale
        } else {
            Self::Current
        })
    }
}

/// Whether an APM task should run on this machine.
///
/// True whenever the symlinks layer ships manifest fragments, or whenever
/// fragments have already been linked into `~/.apm/config/`.  Shared by
/// [`InstallApmPackages`] and [`super::update::UpdateApmPackages`] so both gate
/// on the same "APM is in play here" signal.
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
