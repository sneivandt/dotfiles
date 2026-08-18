//! Task: load and run custom scripts from the overlay repository.
//!
//! [`ReportOverlayScriptSnapshot`] is a lightweight static task that reports
//! how many script tasks were discovered after configuration reload.
//!
//! Each individual script gets its own [`OverlayScriptTask`] created
//! dynamically after [`crate::app::reload::ReloadConfig`]. These tasks appear in
//! the output identically to any other task.

use std::path::PathBuf;

use anyhow::Result;

use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::domains::overlay::resources::script::ScriptResource;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskMeta, TaskResult, TaskStats, TaskVisibility,
    configured_task_result, process_operation,
};
use crate::engine::{IntrinsicState, ResourceChange, ResourceState};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

// ---------------------------------------------------------------------------
// Static task: report discovered scripts
// ---------------------------------------------------------------------------

/// Report overlay script definitions discovered after configuration reload.
///
/// The actual execution of each script is handled by individual
/// [`OverlayScriptTask`] instances injected after configuration reload.
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

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Option<TaskResult> {
        let count = self.config.read().len();
        if count == 0 {
            return None;
        }
        if let Some(name) = announce {
            ctx.log().task_stage(name);
        }
        ctx.log()
            .info(format!("discovered {count} overlay script(s)"));
        Some(TaskResult::Ok)
    }
}

impl Task for ReportOverlayScriptSnapshot {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(REPORT_NAME).with_visibility(TaskVisibility::Internal)
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.overlay().is_some()
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        Ok(self.process(ctx, Some(REPORT_NAME)))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)))
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
}

#[derive(Debug, Clone)]
struct OverlayScriptOperation {
    entry: ScriptEntry,
    overlay_root: PathBuf,
}

impl OverlayScriptOperation {
    const fn new(entry: ScriptEntry, overlay_root: PathBuf) -> Self {
        Self {
            entry,
            overlay_root,
        }
    }

    fn resource(&self, ctx: &Context) -> Result<ScriptResource> {
        ScriptResource::from_entry(&self.entry, &self.overlay_root, ctx.executor_arc())
    }
}

impl Operation for OverlayScriptOperation {
    type Plan = ();

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let resource = self.resource(ctx)?;
        Ok(match resource.current_state()? {
            ResourceState::Correct => OperationState::Complete,
            ResourceState::Missing | ResourceState::Incorrect { .. } => {
                OperationState::needs_run(format!("run {}", self.entry.name), ())
            }
            ResourceState::Invalid { reason } | ResourceState::Unknown { reason } => {
                ctx.log().warn(format!("skipping: {reason}"));
                OperationState::not_applicable(reason)
            }
        })
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        let (_change, output) = self.resource(ctx)?.preview_with_output()?;
        emit_script_lines(ctx, &output, true);
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        let (change, output) = self.resource(ctx)?.apply_with_output()?;
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
            ResourceChange::Applied | ResourceChange::AlreadyCorrect => Ok(TaskResult::Ok),
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
        }
    }
}

fn overlay_script_selector(name: &str) -> String {
    let suffix = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-");
    format!("script-{suffix}")
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

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        ctx.log().task_stage(self.name());
        if let Some(description) = &self.entry.description {
            ctx.log().info(description);
        }
        self.run(ctx).map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(
            ctx,
            &OverlayScriptOperation::new(self.entry.clone(), self.overlay_root.clone()),
        )
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
/// Called from `install.rs` after the configuration-reload boundary to inject
/// dynamic tasks alongside the remaining static tasks.
#[must_use]
pub fn overlay_script_tasks(
    scripts: &[ScriptEntry],
    overlay_root: &std::path::Path,
) -> Vec<Box<dyn Task>> {
    scripts
        .iter()
        .map(|entry| {
            let task: Box<dyn Task> = Box::new(OverlayScriptTask::new(
                entry.clone(),
                overlay_root.to_path_buf(),
            ));
            task
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/scripts.rs"]
mod tests;
