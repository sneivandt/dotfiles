//! APM install task: merge fragments, write the generated manifest, and run
//! `apm install` without advancing dependency refs.

use std::path::Path;

use anyhow::{Context as _, Result};

use super::autopilot::{apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids};
use super::commands::{
    ApmCommand, ensure_copilot_app_enabled, install_task_result, run_apm_command,
};
use super::fragments::{discover_fragment_files, discover_yaml_files, merge_fragments};
use super::manifest::{
    describe_dependencies, manifest_fingerprint, manifest_marker_matches,
    merged_manifest_needs_write, write_manifest_marker, write_merged_manifest,
};
use super::skip_with_warning;
use super::targets::{ApmTargets, missing_apm_reason};
use crate::engine::{Context, Domain, Task, TaskPhase, TaskResult, task_metadata};

/// Converge AI plugin manifests via Microsoft APM.
///
/// Merges the manifest fragments and runs `apm install`, redeploying locally
/// symlinked plugin edits.  It never advances locked dependency refs — that is
/// [`super::update::UpdateApmPackages`]'s job under the `update` command.
#[derive(Debug)]
pub struct InstallApmPackages;

impl Task for InstallApmPackages {
    task_metadata! {
        name: "Install APM packages",
        phase: TaskPhase::Provision,
        domain: Domain::Ai,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.dry_run && !ctx.executor.which("apm") {
            return Ok(skip_with_warning(ctx, missing_apm_reason(ctx)));
        }

        let fragments = discover_fragment_files(&ctx.home)?;
        if fragments.is_empty() {
            return Ok(skip_with_warning(
                ctx,
                "no manifest fragments found under ~/.apm/config/",
            ));
        }

        let manifest_path = ctx.home.join(".apm").join("apm.yml");
        let lock_path = ctx.home.join(".apm").join("apm.lock.yaml");
        let marker_path = ctx.home.join(".apm").join(".dotfiles-manifest.sha256");
        let merged = merge_fragments(&fragments)?;
        let manifest_hash = manifest_fingerprint(&merged);
        let state = ApmInstallState::detect(
            &manifest_path,
            &lock_path,
            &marker_path,
            &merged,
            &manifest_hash,
        )?;
        let targets = ApmTargets::detect(ctx)?;

        if ctx.dry_run {
            let would_change = preview_install(
                ctx,
                targets,
                state,
                fragments.len(),
                &manifest_path,
                &lock_path,
            );
            return Ok(if would_change {
                TaskResult::DryRun
            } else {
                TaskResult::Ok
            });
        }

        let pre_workflows = targets
            .includes_copilot_app()
            .then(|| snapshot_desired_apm_workflow_ids(ctx));
        if targets.includes_copilot_app() {
            ensure_copilot_app_enabled(ctx);
        }

        let manifest_changed = state.manifest_changed();
        if state.manifest_needs_write {
            write_merged_manifest(&manifest_path, &merged)?;
        }

        // Always (re)run `apm install` so locally symlinked plugin edits are
        // redeployed to their runtime locations.  When the manifest changed
        // this also installs newly declared dependencies; on an unchanged
        // manifest it is a quiet idempotent redeploy.
        let install_result =
            install_task_result(run_apm_command(ctx, ApmCommand::Install, targets)?);
        if !matches!(install_result, TaskResult::Ok) {
            // Auth skip (or similar): do not record the manifest as installed
            // and do not attempt to advance dependencies.
            return Ok(install_result);
        }
        if manifest_changed {
            ctx.log.always(&format!(
                "    installed: {}",
                describe_dependencies(&merged)
            ));
        }
        write_manifest_marker(&marker_path, &manifest_hash)?;

        // Convergence is complete.  Advancing locked dependency refs
        // (`apm outdated` / `apm update`) is a separate concern handled by
        // the `update`-only task, so this task never moves a locked ref forward.
        if let Some(pre) = pre_workflows {
            apply_workflow_autopilot_fixup(ctx, &pre);
        }
        Ok(if manifest_changed {
            TaskResult::OkWithMessage(format!("installed {}", describe_dependencies(&merged)))
        } else {
            TaskResult::Ok
        })
    }
}

fn preview_install(
    ctx: &Context,
    targets: ApmTargets,
    state: ApmInstallState,
    fragment_count: usize,
    manifest_path: &Path,
    lock_path: &Path,
) -> bool {
    if !state.manifest_changed() {
        ctx.log
            .debug("APM manifest, lockfile, and install marker are already current");
        return false;
    }

    if targets.includes_copilot_app() {
        ctx.log
            .dry_run("run apm experimental enable copilot-app (idempotent) before install");
        ctx.log.dry_run(
            "re-assert apm-managed Copilot App workflows to autopilot + enabled in \
             ~/.copilot/data.db after a successful install",
        );
    }
    if state.manifest_needs_write {
        ctx.log.dry_run(&format!(
            "merge {fragment_count} APM manifest fragment(s) into {}",
            manifest_path.display()
        ));
        ctx.log.dry_run(&format!(
            "run apm install -g --target {} to sync changed manifest",
            targets.as_str()
        ));
    } else if state.lock_missing {
        ctx.log.dry_run(&format!(
            "run apm install -g --target {} because {} is missing",
            targets.as_str(),
            lock_path.display()
        ));
    } else if state.marker_missing_or_stale {
        ctx.log.dry_run(&format!(
            "run apm install -g --target {} because the current manifest has not been installed \
             successfully yet",
            targets.as_str()
        ));
    } else {
        ctx.log.dry_run(&format!(
            "run apm install -g --target {} to redeploy current manifest content",
            targets.as_str()
        ));
    }
    true
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
    /// The success marker is missing or does not match the current manifest.
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

    /// Whether `apm install` will materially change locked or installed state.
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
            ctx.log.warn(&format!(
                "could not inspect symlinks/apm/config; task will run to avoid hiding the \
                 error: {err:#}"
            ));
            return true;
        }
    }

    match discover_fragment_files(&ctx.home) {
        Ok(fragments) => !fragments.is_empty(),
        Err(err) => {
            ctx.log.warn(&format!(
                "could not inspect ~/.apm/config; task will run to surface the error: {err:#}"
            ));
            true
        }
    }
}
