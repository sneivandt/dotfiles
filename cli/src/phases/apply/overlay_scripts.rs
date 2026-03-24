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
use crate::phases::{Context, Task, TaskPhase, TaskResult, task_deps};
use crate::resources::script::ScriptResource;
use crate::resources::{Resource, ResourceChange, ResourceState};

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
    fn name(&self) -> &'static str {
        "Load overlay scripts"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Repository
    }

    task_deps![crate::phases::repository::reload_config::ReloadConfig];

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
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().overlay.is_some()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        ctx.log.stage(self.name());
        self.run(ctx).map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resource =
            ScriptResource::from_entry(&self.entry, &self.overlay_root, Arc::clone(&ctx.executor));

        let state = resource.current_state()?;
        match state {
            ResourceState::Correct => {
                ctx.log.debug("already correct");
                Ok(TaskResult::Ok)
            }
            ResourceState::Missing | ResourceState::Incorrect { .. } => {
                if ctx.dry_run {
                    let output = resource.dry_run_output()?;
                    emit_script_lines(ctx, &output, true);
                    Ok(TaskResult::DryRun)
                } else {
                    let (change, output) = resource.apply_verbose()?;
                    emit_script_lines(ctx, &output, false);
                    match change {
                        ResourceChange::Skipped { reason } => {
                            ctx.log.warn(&format!("skipping: {reason}"));
                            Ok(TaskResult::Skipped(reason))
                        }
                        _ => Ok(TaskResult::Ok),
                    }
                }
            }
            ResourceState::Invalid { reason } | ResourceState::Unknown { reason } => {
                ctx.log.warn(&format!("skipping: {reason}"));
                Ok(TaskResult::NotApplicable(reason))
            }
        }
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
            Box::new(OverlayScriptTask::new(
                entry.clone(),
                overlay_root.to_path_buf(),
            )) as Box<dyn Task>
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::scripts::ScriptEntry;
    use crate::phases::Task;
    use crate::phases::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn load_should_run_false_without_overlay() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!LoadOverlayScripts.should_run(&ctx));
    }

    #[test]
    fn load_should_run_true_with_overlay() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.overlay = Some(PathBuf::from("/overlay"));
        let ctx = make_linux_context(config);
        assert!(LoadOverlayScripts.should_run(&ctx));
    }

    #[test]
    fn script_task_name_matches_entry() {
        let entry = ScriptEntry {
            name: "Setup database".to_string(),
            path: "scripts/setup-db.ps1".to_string(),
            description: None,
        };
        let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
        assert_eq!(task.name(), "Setup database");
    }

    #[test]
    fn script_task_should_run_false_without_overlay() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "scripts/test.sh".to_string(),
            description: None,
        };
        let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn script_task_should_run_true_with_overlay() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.overlay = Some(PathBuf::from("/overlay"));
        let ctx = make_linux_context(config);
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "scripts/test.sh".to_string(),
            description: None,
        };
        let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn overlay_script_tasks_creates_one_per_entry() {
        let scripts = vec![
            ScriptEntry {
                name: "A".to_string(),
                path: "a.sh".to_string(),
                description: None,
            },
            ScriptEntry {
                name: "B".to_string(),
                path: "b.ps1".to_string(),
                description: None,
            },
        ];
        let tasks = overlay_script_tasks(&scripts, std::path::Path::new("/overlay"));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name(), "A");
        assert_eq!(tasks[1].name(), "B");
    }
}
