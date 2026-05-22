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
use crate::resources::{IntrinsicState, ResourceChange, ResourceState};

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

    /// Returns a per-instance [`TaskId::Dynamic`](crate::phases::TaskId::Dynamic) derived from the script's
    /// name and path.
    ///
    /// Multiple `OverlayScriptTask` instances share the same Rust type, so
    /// the default `TypeId`-based identity would collide in the dependency
    /// graph.  Using a hash of the instance data gives each script a distinct
    /// identity while keeping it deterministic across runs.
    fn task_id(&self) -> crate::phases::TaskId {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.entry.name.hash(&mut h);
        self.entry.path.hash(&mut h);
        crate::phases::TaskId::Dynamic(h.finish())
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
            ScriptResource::from_entry(&self.entry, &self.overlay_root, Arc::clone(&ctx.executor))?;

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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::config::scripts::ScriptEntry;
    use crate::exec::{ExecResult, MockExecutor};
    use crate::phases::test_helpers::{empty_config, make_context, make_linux_context};
    use crate::phases::{Context, Task, TaskResult};
    use crate::platform::{Os, Platform};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn script_entry(name: &str, path: &str) -> ScriptEntry {
        ScriptEntry {
            name: name.to_string(),
            path: path.to_string(),
            description: None,
        }
    }

    fn exec_result(stdout: &str, success: bool, code: Option<i32>) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            success,
            code,
        }
    }

    fn shell_script_fixture() -> (tempfile::TempDir, ScriptEntry, String) {
        let overlay = tempfile::tempdir().expect("create overlay dir");
        let script_path = overlay.path().join("scripts/test.sh");
        std::fs::create_dir_all(script_path.parent().expect("script parent"))
            .expect("create scripts dir");
        std::fs::write(&script_path, "#!/bin/sh\n").expect("write script");
        let script_arg = script_path.display().to_string();
        (
            overlay,
            script_entry("Setup test", "scripts/test.sh"),
            script_arg,
        )
    }

    fn context_with_executor(overlay: &Path, executor: MockExecutor) -> Context {
        let mut config = empty_config(overlay.to_path_buf());
        config.overlay = Some(overlay.to_path_buf());
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

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
        let tasks = overlay_script_tasks(&scripts, Path::new("/overlay"));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name(), "A");
        assert_eq!(tasks[1].name(), "B");
    }

    #[test]
    fn overlay_script_tasks_have_unique_task_ids() {
        // Multiple OverlayScriptTask instances must produce distinct TaskIds so
        // the parallel scheduler does not report a false "dependency cycle".
        use crate::phases::TaskId;
        use std::collections::HashSet;

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
            ScriptEntry {
                name: "C".to_string(),
                path: "c.sh".to_string(),
                description: None,
            },
        ];
        let tasks = overlay_script_tasks(&scripts, Path::new("/overlay"));
        let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TaskId> = ids.iter().copied().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "all overlay script task IDs must be distinct"
        );
        // Each id must be the Dynamic variant.
        assert!(
            ids.iter().all(|id| matches!(id, TaskId::Dynamic(_))),
            "overlay script tasks should use TaskId::Dynamic, not TaskId::Type"
        );
    }

    #[test]
    fn script_task_run_is_ok_when_check_reports_correct() {
        let (overlay, entry, script_arg) = shell_script_fixture();
        let overlay_path = overlay.path().to_path_buf();
        let check_script = script_arg;
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in()
            .once()
            .withf(move |dir, program, args| {
                dir == overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 2
                    && args[0] == check_script.as_str()
                    && args[1] == "--check"
            })
            .returning(|_, _, _| Ok(exec_result("", true, Some(0))));

        let ctx = context_with_executor(overlay.path(), mock);
        let task = OverlayScriptTask::new(entry, overlay.path().to_path_buf());

        assert!(matches!(task.run(&ctx).unwrap(), TaskResult::Ok));
    }

    #[test]
    fn script_task_run_applies_when_check_reports_missing() {
        let (overlay, entry, script_arg) = shell_script_fixture();
        let overlay_path = overlay.path().to_path_buf();
        let check_script = script_arg.clone();
        let apply_overlay_path = overlay.path().to_path_buf();
        let apply_script = script_arg;
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in()
            .once()
            .withf(move |dir, program, args| {
                dir == overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 2
                    && args[0] == check_script.as_str()
                    && args[1] == "--check"
            })
            .returning(|_, _, _| Ok(exec_result("", false, Some(1))));
        mock.expect_run_in()
            .once()
            .withf(move |dir, program, args| {
                dir == apply_overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 1
                    && args[0] == apply_script.as_str()
            })
            .returning(|_, _, _| Ok(exec_result("applied\n", true, Some(0))));

        let ctx = context_with_executor(overlay.path(), mock);
        let task = OverlayScriptTask::new(entry, overlay.path().to_path_buf());

        assert!(matches!(task.run(&ctx).unwrap(), TaskResult::Ok));
    }

    #[test]
    fn script_task_run_uses_dry_run_script_when_context_is_dry_run() {
        let (overlay, entry, script_arg) = shell_script_fixture();
        let overlay_path = overlay.path().to_path_buf();
        let check_script = script_arg.clone();
        let dry_run_overlay_path = overlay.path().to_path_buf();
        let dry_run_script = script_arg;
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in()
            .once()
            .withf(move |dir, program, args| {
                dir == overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 2
                    && args[0] == check_script.as_str()
                    && args[1] == "--check"
            })
            .returning(|_, _, _| Ok(exec_result("", false, Some(1))));
        mock.expect_run_in()
            .once()
            .withf(move |dir, program, args| {
                dir == dry_run_overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 2
                    && args[0] == dry_run_script.as_str()
                    && args[1] == "--dryrun"
            })
            .returning(|_, _, _| Ok(exec_result("would apply\n", true, Some(0))));

        let ctx = context_with_executor(overlay.path(), mock).with_dry_run(true);
        let task = OverlayScriptTask::new(entry, overlay.path().to_path_buf());

        assert!(matches!(task.run(&ctx).unwrap(), TaskResult::DryRun));
    }

    #[test]
    fn script_task_run_treats_check_failures_as_not_applicable() {
        let (overlay, entry, script_arg) = shell_script_fixture();
        let overlay_path = overlay.path().to_path_buf();
        let check_script = script_arg;
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked_in()
            .once()
            .withf(move |dir, program, args| {
                dir == overlay_path.as_path()
                    && program == "sh"
                    && args.len() == 2
                    && args[0] == check_script.as_str()
                    && args[1] == "--check"
            })
            .returning(|_, _, _| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: "boom".to_string(),
                    success: false,
                    code: Some(2),
                })
            });

        let ctx = context_with_executor(overlay.path(), mock);
        let task = OverlayScriptTask::new(entry, overlay.path().to_path_buf());
        let result = task.run(&ctx).unwrap();

        assert!(matches!(
            result,
            TaskResult::NotApplicable(reason) if reason.contains("exit 2") && reason.contains("boom")
        ));
    }

    #[test]
    fn script_task_run_is_not_applicable_when_script_is_missing() {
        let overlay = tempfile::tempdir().expect("create overlay dir");
        let mock = MockExecutor::new();
        let ctx = context_with_executor(overlay.path(), mock);
        let task = OverlayScriptTask::new(
            script_entry("Missing script", "scripts/missing.sh"),
            overlay.path().to_path_buf(),
        );
        let result = task.run(&ctx).unwrap();

        assert!(matches!(
            result,
            TaskResult::NotApplicable(reason) if reason.contains("script not found")
        ));
    }
}
