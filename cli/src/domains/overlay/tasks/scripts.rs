//! Task: load and run custom scripts from the overlay repository.
//!
//! [`ReportOverlayScriptSnapshot`] is a lightweight static task that reports
//! how many script tasks were created from the startup configuration snapshot.
//!
//! Each individual script gets its own [`OverlayScriptTask`] created
//! dynamically at startup (in `install.rs`).  These tasks appear in the
//! output identically to any other task.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::domains::overlay::resources::script::ScriptResource;
use crate::engine::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, process_operation,
};
use crate::engine::{IntrinsicState, ResourceChange, ResourceState};
use crate::runtime::ConfigHandle;

// ---------------------------------------------------------------------------
// Static task: report the startup snapshot
// ---------------------------------------------------------------------------

/// Report overlay script definitions captured at startup.
///
/// Dynamic script tasks cannot be rebuilt after repository synchronization, so
/// this task reports the startup snapshot rather than implying that scripts
/// were rediscovered during configuration reload. The actual execution of each
/// script is handled by individual [`OverlayScriptTask`] instances.
#[derive(Debug)]
pub struct ReportOverlayScriptSnapshot {
    config: ConfigHandle<Vec<ScriptEntry>>,
}

impl ReportOverlayScriptSnapshot {
    /// Create the task with a handle to the overlay script configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<ScriptEntry>>) -> Self {
        Self { config }
    }
}

impl Task for ReportOverlayScriptSnapshot {
    fn name(&self) -> &'static str {
        "Report overlay script snapshot"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Sync
    }

    fn domain(&self) -> Domain {
        Domain::Overlay
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.overlay().is_some()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        let scripts = self.config.read();
        if ctx.overlay().is_none() || scripts.is_empty() {
            return Ok(None);
        }
        let count = scripts.len();
        ctx.log.stage(self.name());
        ctx.log.info(&format!(
            "using {count} overlay script(s) captured at startup"
        ));
        Ok(Some(TaskResult::Ok))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.overlay().is_none() {
            return Ok(TaskResult::NotApplicable(
                "no overlay configured".to_string(),
            ));
        }
        let scripts = self.config.read();
        if scripts.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        let count = scripts.len();
        ctx.log.info(&format!(
            "using {count} overlay script(s) captured at startup"
        ));
        Ok(TaskResult::Ok)
    }
}

// ---------------------------------------------------------------------------
// Dynamic task: one per overlay script entry
// ---------------------------------------------------------------------------

/// A dynamically created task that runs a single overlay script.
///
/// Instances are created at startup from the loaded configuration and
/// injected into the task list so they appear in the output like any
/// other task.
#[derive(Debug)]
pub struct OverlayScriptTask {
    entry: ScriptEntry,
    overlay_root: PathBuf,
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
        ScriptResource::from_entry(&self.entry, &self.overlay_root, Arc::clone(&ctx.executor))
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
                ctx.log.warn(&format!("skipping: {reason}"));
                OperationState::not_applicable(reason)
            }
        })
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        let (_change, output) = self.resource(ctx)?.preview_with_output()?;
        emit_script_lines(ctx, &output, true);
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        let (change, output) = self.resource(ctx)?.apply_with_output()?;
        emit_script_lines(ctx, &output, false);
        match change {
            ResourceChange::Skipped { reason } => {
                ctx.log.warn(&format!("skipping: {reason}"));
                Ok(TaskResult::Skipped(reason))
            }
            ResourceChange::Applied | ResourceChange::AlreadyCorrect => Ok(TaskResult::Ok),
        }
    }
}

impl OverlayScriptTask {
    /// Create a new overlay script task.
    #[must_use]
    pub const fn new(entry: ScriptEntry, overlay_root: PathBuf) -> Self {
        Self {
            entry,
            overlay_root,
        }
    }
}

impl Task for OverlayScriptTask {
    fn name(&self) -> &str {
        &self.entry.name
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::Overlay
    }

    /// Returns a per-instance [`TaskId::Dynamic`](crate::engine::TaskId::Dynamic) derived from the script's
    /// name and path.
    ///
    /// Multiple `OverlayScriptTask` instances share the same Rust type, so
    /// the default `TypeId`-based identity would collide in the dependency
    /// graph.  Using a hash of the instance data gives each script a distinct
    /// identity while keeping it deterministic across runs.
    fn task_id(&self) -> crate::engine::TaskId {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.entry.name.hash(&mut h);
        self.entry.path.hash(&mut h);
        crate::engine::TaskId::Dynamic(h.finish())
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.overlay().is_some()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        ctx.log.stage(self.name());
        if let Some(description) = &self.entry.description {
            ctx.log.info(description);
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
                ctx.log.dry_run(line);
            } else {
                ctx.log.always(line);
            }
        }
    }
}

/// Create [`OverlayScriptTask`] instances for every script in the config.
///
/// Called from `install.rs` after config is loaded to inject dynamic tasks
/// into the task list alongside the static ones.
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
