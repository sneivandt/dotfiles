//! Task: load and run custom scripts from the overlay repository.
//!
//! [`LoadOverlayScripts`] is a lightweight static task that validates the
//! overlay is configured and reports how many scripts were discovered.
//!
//! Each individual script gets its own [`OverlayScriptTask`] created
//! dynamically at startup (in `install.rs`).  These tasks appear in the
//! output identically to any other task.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::config::scripts::ScriptEntry;
use crate::resources::script::{self, ScriptResource};
use crate::resources::{IntrinsicState, ResourceChange, ResourceState};
use crate::tasks::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, process_operation,
    task_metadata,
};

// ---------------------------------------------------------------------------
// Static task: Load overlay scripts
// ---------------------------------------------------------------------------

/// Load overlay script definitions from configuration.
///
/// This task validates that an overlay is configured and logs the number of
/// script entries discovered.  The actual execution of each script is handled
/// by individual [`OverlayScriptTask`] instances.
#[derive(Debug)]
pub struct LoadOverlayScripts;

impl Task for LoadOverlayScripts {
    task_metadata! {
        name: "Load overlay scripts",
        phase: TaskPhase::Sync,
        domain: Domain::Overlay,
        deps: [crate::tasks::repository::reload_config::ReloadConfig],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().overlay.is_some()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        let config = ctx.config_read();
        if config.overlay.is_none() || config.scripts.is_empty() {
            return Ok(None);
        }
        let count = config.scripts.len();
        ctx.log.stage(self.name());
        ctx.log
            .info(&format!("discovered {count} overlay script(s)"));
        Ok(Some(TaskResult::Ok))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = ctx.config_read();
        if config.overlay.is_none() {
            return Ok(TaskResult::NotApplicable(
                "no overlay configured".to_string(),
            ));
        }
        if config.scripts.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        let count = config.scripts.len();
        ctx.log
            .info(&format!("discovered {count} overlay script(s)"));
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

    fn run_script(&self, ctx: &Context, mode: ScriptMode) -> Result<(ResourceChange, String)> {
        let script_path =
            crate::config::scripts::resolve_script_path(&self.entry, &self.overlay_root)?;
        if !script_path.exists() {
            return Ok((
                ResourceChange::Skipped {
                    reason: format!("script not found: {}", script_path.display()),
                },
                String::new(),
            ));
        }

        script::ensure_script_path_within(&self.overlay_root, &script_path)?;
        let (interpreter, mut args) = script::interpreter_args_for(&script_path, &*ctx.executor)?;
        let script_str = script_path.display().to_string();
        args.push(&script_str);
        if let Some(flag) = mode.flag() {
            args.push(flag);
        }

        let action = mode.action();
        let result = ctx
            .executor
            .run_in(&self.overlay_root, interpreter, &args)
            .map_err(|err| err.context(format!("{action} script: {}", self.entry.name)))?;

        Ok((ResourceChange::Applied, result.stdout))
    }
}

#[derive(Debug, Clone, Copy)]
enum ScriptMode {
    Apply,
    DryRun,
}

impl ScriptMode {
    const fn flag(self) -> Option<&'static str> {
        match self {
            Self::Apply => None,
            Self::DryRun => Some("--dryrun"),
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Apply => "running",
            Self::DryRun => "dry-run",
        }
    }
}

impl Operation for OverlayScriptOperation {
    fn current_state(&self, ctx: &Context) -> Result<OperationState> {
        let resource = self.resource(ctx)?;
        Ok(match resource.current_state()? {
            ResourceState::Correct => OperationState::Complete,
            ResourceState::Missing | ResourceState::Incorrect { .. } => {
                OperationState::needs_run(format!("run {}", self.entry.name))
            }
            ResourceState::Invalid { reason } | ResourceState::Unknown { reason } => {
                ctx.log.warn(&format!("skipping: {reason}"));
                OperationState::not_applicable(reason)
            }
        })
    }

    fn preview(&self, ctx: &Context, _state: &OperationState) -> Result<TaskResult> {
        let (_change, output) = self.run_script(ctx, ScriptMode::DryRun)?;
        emit_script_lines(ctx, &output, true);
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _state: &OperationState) -> Result<TaskResult> {
        let (change, output) = self.run_script(ctx, ScriptMode::Apply)?;
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

    /// Returns a per-instance [`TaskId::Dynamic`](crate::tasks::TaskId::Dynamic) derived from the script's
    /// name and path.
    ///
    /// Multiple `OverlayScriptTask` instances share the same Rust type, so
    /// the default `TypeId`-based identity would collide in the dependency
    /// graph.  Using a hash of the instance data gives each script a distinct
    /// identity while keeping it deterministic across runs.
    fn task_id(&self) -> crate::tasks::TaskId {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.entry.name.hash(&mut h);
        self.entry.path.hash(&mut h);
        crate::tasks::TaskId::Dynamic(h.finish())
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().overlay.is_some()
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
