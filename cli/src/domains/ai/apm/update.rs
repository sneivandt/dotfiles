//! APM update task: advance locked dependency refs for the `update` command.

use std::path::Path;

use super::ApmFragmentSource;
use super::commands::{ApmCommand, ApmCommandResult, ApmOutdatedResult, check_apm_outdated};
use super::fragments::{discover_effective_fragment_files, merge_fragments};
use super::install::apm_task_should_run;
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use super::manifest::manifest_marker_matches;
use super::skip;
use super::sources::install_fingerprint;
use super::targets::missing_apm_reason;
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;
use anyhow::{Context as _, Result};

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
        let system = ctx.system();
        let home = system.home();
        if !system.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let fragments = discover_effective_fragment_files(home, &self.fragments.read())?;
        if fragments.is_empty() {
            return Ok(skip("no manifest fragments found under ~/.apm/config/"));
        }

        // Re-assert the convergence precondition: only advance locked refs when
        // the current merged manifest has been installed successfully.  This
        // preserves the "advance only after a successful install" invariant.
        let lock_path = home.join(".apm").join("apm.lock.yaml");
        let lock_present = lock_path
            .try_exists()
            .with_context(|| format!("checking APM lockfile {}", lock_path.display()))?;
        let merged = merge_fragments(&fragments)?;
        let targets = ManagedTargets::detect(ctx)?;
        let manifest_hash = install_fingerprint(&merged, home, targets.active())?;
        let marker_path = home.join(".apm").join(".dotfiles-manifest.sha256");
        let marker_matches = manifest_marker_matches(&marker_path, &manifest_hash)?;
        if !lock_present || !marker_matches {
            let reason = "APM manifest has not been installed successfully yet; skipping \
                          dependency advancement"
                .to_string();
            ctx.log().debug(&reason);
            return Ok(TaskResult::Skipped(reason));
        }

        match check_apm_outdated(ctx)? {
            status @ (ApmOutdatedResult::Outdated | ApmOutdatedResult::Unknown)
                if ctx.dry_run() =>
            {
                if matches!(status, ApmOutdatedResult::Unknown) {
                    ctx.log().debug(
                        "APM could not determine whether remote dependency updates are available; \
                         conservatively previewing an update",
                    );
                }
                Ok(preview_apm_update(ctx, targets))
            }
            ApmOutdatedResult::Outdated | ApmOutdatedResult::Unknown => {
                advance_apm_dependencies(ctx, targets)
            }
            ApmOutdatedResult::Current => Ok(TaskResult::Ok),
            ApmOutdatedResult::AuthSkipped(reason) => Ok(TaskResult::Skipped(reason)),
        }
    }
}

fn preview_apm_update(ctx: &Context, targets: ManagedTargets) -> TaskResult {
    ctx.log().dry_run(
        "run apm update -g --yes with manifest-resolved runtimes; APM skips dependencies already \
         at their latest matching refs",
    );
    targets.preview(ctx, ManagedTargetPreview::Update);
    TaskResult::DryRun
}

/// Advance locked user-scope dependencies to the latest matching refs.
///
/// Runs only under the `update` command. APM's update command is idempotent, so
/// it runs directly and the lockfile determines whether any ref advanced.
fn advance_apm_dependencies(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
    let target_snapshot = targets.snapshot(ctx);
    let result = match run_apm_update(ctx, targets)? {
        ApmUpdateOutcome::Changed => {
            // Run log only: the status row already carries this as its reason.
            ctx.log().trace("updated: advanced to latest versions");
            TaskStats::changed_with_message("advanced APM dependencies to latest versions").finish()
        }
        ApmUpdateOutcome::Unchanged => {
            ctx.log().debug("APM dependencies already at latest refs");
            TaskResult::Ok
        }
        ApmUpdateOutcome::Skipped(reason) => return Ok(TaskResult::Skipped(reason)),
    };
    targets.finish(ctx, &target_snapshot);
    Ok(result)
}

/// Refresh locked user-scope dependencies to the latest matching refs.
///
/// Detects whether anything actually advanced by snapshotting the APM lockfile
/// (`~/.apm/apm.lock.yaml`) before and after the run rather than parsing console
/// output. Dependency state in the lockfile only changes when a pinned ref
/// actually advances, making it the authoritative change signal — provided the
/// volatile bookkeeping keys are normalized away first (see
/// [`normalize_lock_snapshot`]).
fn run_apm_update(ctx: &Context, targets: ManagedTargets) -> Result<ApmUpdateOutcome> {
    let lock_path = ctx.home().join(".apm").join("apm.lock.yaml");
    let lock_before = read_lock_snapshot(&lock_path)?;
    match targets.run_apm_command(ctx, ApmCommand::Update)? {
        ApmCommandResult::Success => {
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

/// Top-level lockfile keys unrelated to dependency resolution.
///
/// Explicit target redeploys rewrite deployment and MCP ledgers even when no
/// package ref advances, so those ledgers must not drive the update result.
const VOLATILE_LOCK_KEYS: &[&str] = &[
    "generated_at",
    "deployments",
    "mcp_servers",
    "mcp_configs",
    "mcp_target_servers",
];
const VOLATILE_DEPENDENCY_KEYS: &[&str] = &["deployed_files", "deployed_file_hashes"];

/// Read the APM lockfile for before/after change detection.
///
/// A missing lockfile is represented as `None`; other errors are surfaced.
fn read_lock_snapshot(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(normalize_lock_snapshot(&String::from_utf8_lossy(
            &bytes,
        )))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading APM lockfile {}", path.display())),
    }
}

/// Strip volatile bookkeeping keys so only real dependency state is compared.
///
/// Falls back to the raw text whenever the lockfile cannot be parsed or
/// re-serialized as YAML: an unparseable lockfile still compares
/// deterministically, it just keeps the old byte-for-byte semantics.
pub(super) fn normalize_lock_snapshot(text: &str) -> String {
    use serde_yaml_ng::Value;

    let Ok(mut value) = serde_yaml_ng::from_str::<Value>(text) else {
        return text.to_owned();
    };
    let Some(mapping) = value.as_mapping_mut() else {
        return text.to_owned();
    };
    for key in VOLATILE_LOCK_KEYS {
        mapping.remove(Value::String((*key).to_owned()));
    }
    if let Some(dependencies) = mapping
        .get_mut(Value::String("dependencies".to_owned()))
        .and_then(Value::as_sequence_mut)
    {
        for dependency in dependencies {
            let Some(dependency) = dependency.as_mapping_mut() else {
                continue;
            };
            for key in VOLATILE_DEPENDENCY_KEYS {
                dependency.remove(Value::String((*key).to_owned()));
            }
        }
    }
    serde_yaml_ng::to_string(&value).unwrap_or_else(|_| text.to_owned())
}
