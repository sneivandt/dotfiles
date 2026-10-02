//! Task: load and run custom scripts from the overlay repository.
//!
//! [`ReportOverlayScriptSnapshot`] is a lightweight static task that reports
//! how many script tasks were discovered at startup.
//!
//! Each individual script gets its own [`OverlayScriptTask`] when the command
//! builds its task set. These tasks appear in the output identically to any
//! other task.

use std::path::PathBuf;

use anyhow::Result;

use crate::domains::overlay::config::scripts::{ScriptEntry, overlay_script_selector};
use crate::domains::overlay::resources::script::ScriptResource;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskMeta, TaskResult, TaskStats, TaskVisibility,
    process_operation,
};
use crate::engine::{IntrinsicState, ResourceChange, ResourceState};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

// ---------------------------------------------------------------------------
// Static task: report discovered scripts
// ---------------------------------------------------------------------------

/// Report overlay script definitions discovered at startup.
///
/// The actual execution of each script is handled by individual
/// [`OverlayScriptTask`] instances created from the same configuration snapshot.
#[derive(Debug)]
pub struct ReportOverlayScriptSnapshot {
    config: ConfigHandle<Vec<ScriptEntry>>,
}

const REPORT_NAME: &str = "Report overlay scripts";

impl ReportOverlayScriptSnapshot {
    /// Create the task with a handle to the overlay script configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<ScriptEntry>>) -> Self {
        Self { config }
    }
}

impl Task for ReportOverlayScriptSnapshot {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(REPORT_NAME).with_visibility(TaskVisibility::Internal)
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.overlay().is_some()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let count = self.config.get().len();
        if count == 0 {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        ctx.log()
            .info(format!("discovered {count} overlay script(s)"));
        Ok(TaskResult::Ok)
    }
}

// ---------------------------------------------------------------------------
// Dynamic task: one per overlay script entry
// ---------------------------------------------------------------------------

/// A dynamically created task that runs a single overlay script.
///
/// Instances are created after configuration synchronization and injected into
/// the task list so they appear in the output like any other task.
#[derive(Debug)]
pub struct OverlayScriptTask {
    entry: ScriptEntry,
    overlay_root: PathBuf,
    selector: String,
    mode: ScriptTaskMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptTaskMode {
    Apply,
    Remove,
}

impl Operation for OverlayScriptTask {
    type Plan = ();

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let resource = self.resource(ctx)?;
        Ok(match (self.mode, resource.current_state()?) {
            (ScriptTaskMode::Apply, ResourceState::Correct)
            | (ScriptTaskMode::Remove, ResourceState::Missing) => OperationState::Complete,
            (ScriptTaskMode::Apply, ResourceState::Missing | ResourceState::Incorrect { .. })
            | (ScriptTaskMode::Remove, ResourceState::Correct | ResourceState::Incorrect { .. }) => {
                OperationState::needs_run(())
            }
            (_, ResourceState::Invalid { reason } | ResourceState::Unknown { reason }) => {
                anyhow::bail!(reason)
            }
        })
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        if self.mode == ScriptTaskMode::Remove {
            ctx.log().dry_run("would remove overlay script state");
        } else {
            let (_change, output) = self.resource(ctx)?.preview_with_output()?;
            emit_script_lines(ctx, &output, true);
        }
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        let (change, output) = match self.mode {
            ScriptTaskMode::Apply => self.resource(ctx)?.apply_with_output()?,
            ScriptTaskMode::Remove => self.resource(ctx)?.remove_with_output()?,
        };
        emit_script_lines(ctx, &output, false);
        match change {
            ResourceChange::Skipped { reason, kind } => {
                ctx.log().warn(format!("skipping: {reason}"));
                Ok(if kind.is_failure() {
                    TaskResult::unmet(reason)
                } else {
                    TaskResult::skipped(reason)
                })
            }
            ResourceChange::Applied => Ok(TaskStats::changed().finish()),
            ResourceChange::AlreadyCorrect => Ok(TaskResult::Ok),
        }
    }
}

impl OverlayScriptTask {
    /// Create a new overlay script task.
    #[must_use]
    pub fn new(entry: ScriptEntry, overlay_root: PathBuf) -> Self {
        let selector = overlay_script_selector(&entry.name);
        Self {
            entry,
            overlay_root,
            selector,
            mode: ScriptTaskMode::Apply,
        }
    }

    fn with_mode(entry: ScriptEntry, overlay_root: PathBuf, mode: ScriptTaskMode) -> Self {
        Self {
            mode,
            ..Self::new(entry, overlay_root)
        }
    }

    fn resource(&self, ctx: &Context) -> Result<ScriptResource> {
        ScriptResource::from_entry(&self.entry, &self.overlay_root, ctx.executor_arc())
    }
}

impl Task for OverlayScriptTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(&self.entry.name).with_selector(&self.selector)
    }

    /// Returns a collision-free per-instance dynamic task identity.
    ///
    /// Multiple `OverlayScriptTask` instances share the same Rust type, so
    /// the default `TypeId`-based identity would collide in the dependency
    /// graph. The concrete task type plus the unabridged configured name and
    /// path form a structured identity without relying on a probabilistic hash.
    fn task_id(&self) -> crate::engine::TaskId {
        crate::engine::TaskId::dynamic::<Self>(format!(
            "{}\u{0}{}",
            self.entry.name, self.entry.path
        ))
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.overlay().is_some()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if let Some(description) = &self.entry.description {
            ctx.log().info(description);
        }
        process_operation(ctx, self)
    }
}

/// Forward captured script stdout through the engine logger.
///
/// Each non-empty line is emitted via the appropriate logger method:
/// `dry_run` for dry-run mode, `always` for apply.
fn emit_script_lines(ctx: &Context, output: &str, dry_run: bool) {
    for line in output.lines() {
        if !line.is_empty() {
            if dry_run {
                ctx.log().dry_run(line);
            } else {
                ctx.log().always(line);
            }
        }
    }
}

/// Create [`OverlayScriptTask`] instances for every script in the config.
///
/// Called from `install.rs` during startup to create dynamic tasks alongside
/// the static catalog.
#[must_use]
pub fn overlay_script_tasks(
    scripts: &[ScriptEntry],
    overlay_root: &std::path::Path,
) -> Vec<Box<dyn Task>> {
    script_tasks(scripts, overlay_root, ScriptTaskMode::Apply)
}

/// Create removal tasks for every active overlay script.
#[must_use]
pub fn overlay_script_removal_tasks(
    scripts: &[ScriptEntry],
    overlay_root: &std::path::Path,
) -> Vec<Box<dyn Task>> {
    script_tasks(scripts, overlay_root, ScriptTaskMode::Remove)
}

fn script_tasks(
    scripts: &[ScriptEntry],
    overlay_root: &std::path::Path,
    mode: ScriptTaskMode,
) -> Vec<Box<dyn Task>> {
    scripts
        .iter()
        .map(|entry| {
            let task: Box<dyn Task> = Box::new(OverlayScriptTask::with_mode(
                entry.clone(),
                overlay_root.to_path_buf(),
                mode,
            ));
            task
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/scripts.rs"]
mod tests;
