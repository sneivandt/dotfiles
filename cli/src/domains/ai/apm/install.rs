//! APM install task: merge fragments, write the generated manifest, and run
//! `apm install` without advancing dependency refs.

use std::path::Path;

use anyhow::{Context as _, Result};

use super::autopilot::{apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids};
use super::commands::{
    ApmCommand, ensure_copilot_app_enabled, install_task_result, prune_user_scope, run_apm_command,
};
use super::fragments::{discover_fragment_files, discover_yaml_files, merge_fragments};
use super::manifest::{
    describe_dependencies, manifest_marker_matches, merged_manifest_needs_write,
    write_manifest_marker, write_merged_manifest,
};
use super::skip;
use super::sources::install_fingerprint;
use super::targets::{ApmTargets, missing_apm_reason};
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::logging::OutputExt as _;

/// Converge AI plugin manifests via Microsoft APM.
///
/// Merges the manifest fragments and runs `apm install` when the manifest,
/// local plugin sources, or resolved targets have changed since the last
/// successful install.  It never advances locked dependency refs — that is
/// [`super::update::UpdateApmPackages`]'s job under the `update` command.
#[derive(Debug)]
pub struct InstallApmPackages;

impl Task for InstallApmPackages {
    task_metadata! {
        name: "APM packages",
        selector: "apm",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let system = ctx.system();
        let home = system.home();
        if !ctx.dry_run() && !system.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let fragments = discover_fragment_files(home)?;
        if fragments.is_empty() {
            return Ok(skip("no manifest fragments found under ~/.apm/config/"));
        }

        let manifest_path = home.join(".apm").join("apm.yml");
        let lock_path = home.join(".apm").join("apm.lock.yaml");
        let marker_path = home.join(".apm").join(".dotfiles-manifest.sha256");
        let merged = merge_fragments(&fragments)?;
        let targets = ApmTargets::detect(ctx)?;
        let manifest_hash = install_fingerprint(&merged, home, targets.includes_copilot_app())?;
        let state = ApmInstallState::detect(
            &manifest_path,
            &lock_path,
            &marker_path,
            &merged,
            &manifest_hash,
        )?;
        if ctx.dry_run() {
            if !state.manifest_changed() {
                return Ok(TaskResult::Ok);
            }
            let planned = preview_install(
                ctx,
                targets,
                state,
                fragments.len(),
                &manifest_path,
                &lock_path,
            );
            // Report the planned actions as stats so the run totals read
            // `N changes in 1 task` rather than a bare `would change`.
            return Ok(TaskStats {
                changed: planned,
                ..TaskStats::default()
            }
            .finish());
        }

        // `manifest_hash` covers the merged manifest, the content of every
        // locally symlinked plugin source, and the resolved target set, so a
        // matching marker means the last successful install already deployed
        // exactly this.  Re-running `apm install` would spawn several seconds
        // of subprocesses to reach the state we are already in.
        if !state.manifest_changed() {
            ctx.debug_fmt(|| {
                "APM manifest, local plugin sources, and targets are unchanged since the last \
                 successful install; skipping apm install"
                    .to_string()
            });
            return Ok(TaskResult::Ok);
        }

        let pre_workflows = targets
            .includes_copilot_app()
            .then(|| snapshot_desired_apm_workflow_ids(ctx));
        if targets.includes_copilot_app() {
            ensure_copilot_app_enabled(ctx);
        }

        if state.manifest_needs_write {
            write_merged_manifest(&manifest_path, &merged)?;
        }

        // Reached only when something APM cares about actually changed: a
        // manifest edit, a local plugin edit, a new target, a missing lockfile,
        // or a marker that never recorded a successful install.
        let install_result =
            install_task_result(run_apm_command(ctx, ApmCommand::Install, targets)?);
        if !matches!(install_result, TaskResult::Ok) {
            // Auth skip (or similar): do not record the manifest as installed
            // and do not attempt to advance dependencies.
            return Ok(install_result);
        }
        // Run log only: the task's own status row already reports the same
        // phrase as its reason, so surfacing it as a detail line would state
        // the change twice.
        ctx.log()
            .trace(format!("installed: {}", describe_dependencies(&merged)));
        write_manifest_marker(&marker_path, &manifest_hash)?;
        prune_user_scope(ctx)?;

        // Convergence is complete.  Advancing locked dependency refs
        // is a separate concern handled by the `update`-only task, so this task
        // never moves a locked ref forward.
        if let Some(pre) = pre_workflows {
            apply_workflow_autopilot_fixup(ctx, &pre);
        }
        Ok(
            TaskStats::changed_with_message(format!(
                "installed {}",
                describe_dependencies(&merged)
            ))
            .finish(),
        )
    }
}

/// Emit the dry-run preview for an APM install, returning the number of
/// planned actions so the caller can report accurate change counts.
fn preview_install(
    ctx: &Context,
    targets: ApmTargets,
    state: ApmInstallState,
    fragment_count: usize,
    manifest_path: &Path,
    lock_path: &Path,
) -> u32 {
    let mut planned = 0_u32;
    if state.manifest_needs_write {
        ctx.log().dry_run(format!(
            "merge {fragment_count} APM manifest fragment(s) into {}",
            manifest_path.display()
        ));
        ctx.log()
            .dry_run("run apm install -g with manifest-resolved runtimes to sync changed manifest");
        planned = planned.saturating_add(2);
    } else if state.lock_missing {
        ctx.log().dry_run(format!(
            "run apm install -g with manifest-resolved runtimes because {} is missing",
            lock_path.display()
        ));
        planned = planned.saturating_add(1);
    } else if state.marker_missing_or_stale {
        ctx.log().dry_run(
            "run apm install -g with manifest-resolved runtimes because the current manifest has \
             not been installed successfully yet",
        );
        planned = planned.saturating_add(1);
    }
    if targets.includes_copilot_app() {
        ctx.log().dry_run(
            "run apm install -g --target copilot-app to sync Copilot App workflows separately",
        );
        planned = planned.saturating_add(1);
    }
    planned
}

/// Filesystem-derived signals that decide whether `apm install` must run and
/// what to report.
///
/// Computed once per [`InstallApmPackages::run`] from the merged manifest and
/// the on-disk lockfile/marker so the dry-run preview and the real execution
/// path branch on identical state.
#[derive(Debug, Clone, Copy)]
struct ApmInstallState {
    /// The merged manifest differs from the on-disk `~/.apm/apm.yml`.
    manifest_needs_write: bool,
    /// The APM lockfile is absent (a fresh machine or wiped state).
    lock_missing: bool,
    /// The success marker is missing or does not match the current
    /// manifest, local plugin content, and target set.
    marker_missing_or_stale: bool,
}

impl ApmInstallState {
    /// Detect install state from the merged manifest and on-disk artifacts.
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
        Ok(Self {
            manifest_needs_write,
            lock_missing,
            marker_missing_or_stale,
        })
    }

    /// Whether `apm install` has any work to do.
    ///
    /// False means the marker already records a successful install of exactly
    /// this manifest, plugin content, and target set, so the install is a
    /// no-op worth skipping outright.
    const fn manifest_changed(self) -> bool {
        self.manifest_needs_write || self.lock_missing || self.marker_missing_or_stale
    }
}

/// Whether an APM task should run on this machine.
///
/// True whenever the symlinks layer ships manifest fragments, or whenever
/// fragments have already been linked into `~/.apm/config/`.  Shared by
/// [`InstallApmPackages`] and [`super::update::UpdateApmPackages`] so both gate
/// on the same "APM is in play here" signal.
pub(super) fn apm_task_should_run(ctx: &Context) -> bool {
    let repo_config_dir = ctx.root().join("symlinks").join("apm").join("config");
    match discover_yaml_files(&repo_config_dir) {
        Ok(fragments) if !fragments.is_empty() => return true,
        Ok(_) => {}
        Err(err) => {
            ctx.log().warn(format!(
                "could not inspect symlinks/apm/config; task will run to avoid hiding the \
                 error: {err:#}"
            ));
            return true;
        }
    }

    match discover_fragment_files(ctx.home()) {
        Ok(fragments) => !fragments.is_empty(),
        Err(err) => {
            ctx.log().warn(format!(
                "could not inspect ~/.apm/config; task will run to surface the error: {err:#}"
            ));
            true
        }
    }
}
