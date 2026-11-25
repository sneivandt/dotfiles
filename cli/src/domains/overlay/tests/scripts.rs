//! Unit tests for overlay script tasks.

use super::*;
use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::engine::{Context, Task, TaskResult, execute};
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::logging::{BufferedLog, Logger, TaskStatus};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{CapturingOutput, assert_task_changed, make_context, make_linux_context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn script_entry(name: &str, path: &str) -> ScriptEntry {
    ScriptEntry {
        name: name.to_string(),
        path: path.to_string(),
        description: None,
        check_remove: false,
    }
}

fn context_with_executor(overlay: &Path, executor: MockExecutor) -> Context {
    make_context(
        overlay.to_path_buf(),
        Some(overlay.to_path_buf()),
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
}

type ShellStep = (Option<&'static str>, ExecResult);

struct ShellCase {
    overlay: tempfile::TempDir,
    task: OverlayScriptTask,
    ctx: Context,
}

impl ShellCase {
    fn new(
        mode: ScriptTaskMode,
        check_remove: bool,
        steps: impl IntoIterator<Item = ShellStep>,
    ) -> Self {
        let overlay = tempfile::tempdir().expect("create overlay dir");
        let mut entry = script_entry("Setup test", "scripts/test.sh");
        entry.check_remove = check_remove;
        let script = overlay.path().join(&entry.path);
        std::fs::create_dir_all(script.parent().unwrap()).expect("create scripts dir");
        std::fs::write(&script, "#!/bin/sh\n").expect("write script");
        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        for (flag, result) in steps {
            let root = overlay.path().to_owned();
            let mut args = vec![script.clone().into_os_string()];
            args.extend(flag.map(std::ffi::OsString::from));
            let checked = !matches!(flag, Some("--check" | "--check-remove"));
            mock.expect_execute()
                .once()
                .in_sequence(&mut seq)
                .return_once(move |spec| {
                    assert_eq!(spec.working_dir(), Some(root.as_path()));
                    assert_eq!(spec.program(), "sh");
                    assert_eq!(spec.arguments(), args);
                    assert_eq!(spec.is_checked(), checked);
                    if checked && !result.success {
                        let command = ["sh", "script.sh"]
                            .into_iter()
                            .chain(flag)
                            .collect::<Vec<_>>()
                            .join(" ");
                        Err(ExecError::non_zero(command, result))
                    } else {
                        Ok(result)
                    }
                });
        }
        let root = overlay.path().to_owned();
        let ctx = context_with_executor(&root, mock);
        let task = match mode {
            ScriptTaskMode::Apply => OverlayScriptTask::new(entry, root),
            ScriptTaskMode::Remove => OverlayScriptTask::with_mode(entry, root, mode),
        };
        Self { overlay, task, ctx }
    }

    fn apply(steps: impl IntoIterator<Item = ShellStep>) -> Self {
        Self::new(ScriptTaskMode::Apply, false, steps)
    }

    fn remove(steps: impl IntoIterator<Item = ShellStep>) -> Self {
        Self::new(ScriptTaskMode::Remove, false, steps)
    }

    fn run(&self) -> Result<TaskResult> {
        self.task.run(&self.ctx)
    }
}

fn check(status: i32) -> ShellStep {
    (Some("--check"), probe_result(status))
}

fn probe_result(status: i32) -> ExecResult {
    if status == 0 {
        ExecResult::success("")
    } else {
        ExecResult::failure("", "", Some(status))
    }
}

fn configured_context() -> Context {
    make_linux_context(PathBuf::from("/tmp"), Some(PathBuf::from("/overlay")))
}

#[test]
fn script_task_name_matches_entry() {
    let entry = script_entry("Setup database", "scripts/setup-db.ps1");
    let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
    assert_eq!(task.name(), "Setup database");
    assert_eq!(task.selector(), "script-setup-database");
}

#[test]
fn script_task_should_run_false_without_overlay() {
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    let entry = script_entry("test", "scripts/test.sh");
    let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
    assert!(!task.should_run(&ctx));
}

#[test]
fn script_task_should_run_true_with_overlay() {
    let ctx = configured_context();
    let entry = script_entry("test", "scripts/test.sh");
    let task = OverlayScriptTask::new(entry, PathBuf::from("/overlay"));
    assert!(task.should_run(&ctx));
}

#[test]
fn overlay_script_tasks_creates_one_per_entry() {
    let scripts = [script_entry("A", "a.sh"), script_entry("B", "b.ps1")];
    let tasks = overlay_script_tasks(&scripts, Path::new("/overlay"));
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name(), "A");
    assert_eq!(tasks[1].name(), "B");
}

#[test]
fn removal_task_checks_then_runs_remove_with_output() {
    let case = ShellCase::remove([
        check(0),
        (Some("--remove"), ExecResult::success("removed\n")),
    ]);
    assert_task_changed(&case.run().unwrap());
}

#[test]
fn removal_task_skips_when_script_reports_absent_state() {
    let case = ShellCase::remove([check(1)]);
    assert!(matches!(case.run().unwrap(), TaskResult::Ok));
}

#[test]
fn opt_in_removal_probe_can_detect_partial_state_without_touching_a_preview() {
    for (state, dry_run) in [(0, false), (0, true), (1, false), (42, false)] {
        let mut steps = vec![(Some("--check-remove"), probe_result(state))];
        if state == 0 && !dry_run {
            steps.push((
                Some("--remove"),
                ExecResult::success("removed partial state"),
            ));
        }
        let case = ShellCase::new(ScriptTaskMode::Remove, true, steps);
        let result = case.task.run(&case.ctx.with_dry_run(dry_run));
        match state {
            0 => assert_task_changed(&result.unwrap()),
            1 => assert!(matches!(result.unwrap(), TaskResult::Ok)),
            _ => assert!(result.is_err()),
        }
    }
}

#[test]
fn removal_dry_run_does_not_execute_remove() {
    let case = ShellCase::remove([check(0)]);
    let output = Arc::new(CapturingOutput::default());
    let ctx = case
        .ctx
        .with_dry_run(true)
        .with_log(Arc::<CapturingOutput>::clone(&output));
    assert_task_changed(&case.task.run(&ctx).unwrap());
    let actions = output.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].verb, "remove");
    assert_eq!(actions[0].subject, "overlay script state");
    assert!(actions[0].planned);
    assert_eq!(actions[0].message, "remove overlay script state");
}

#[test]
fn script_disappearing_after_check_fails_apply_preview_and_remove() {
    for (mode, dry_run) in [
        (ScriptTaskMode::Apply, false),
        (ScriptTaskMode::Apply, true),
        (ScriptTaskMode::Remove, false),
    ] {
        let overlay = tempfile::tempdir_in(".").unwrap();
        let script_path = overlay.path().join("test.sh");
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();
        let expected_root = overlay.path().to_path_buf();
        let expected_args = [
            script_path.as_os_str().to_os_string(),
            std::ffi::OsString::from("--check"),
        ];
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "sh"
                    && spec.arguments() == expected_args
                    && spec.working_dir() == Some(expected_root.as_path())
                    && !spec.is_checked()
            })
            .return_once(move |_| {
                std::fs::remove_file(script_path).unwrap();
                Ok(if mode == ScriptTaskMode::Remove {
                    ExecResult::success("")
                } else {
                    ExecResult::failure("", "", Some(1))
                })
            });
        let ctx = context_with_executor(overlay.path(), mock).with_dry_run(dry_run);
        let task = OverlayScriptTask::with_mode(
            script_entry("Setup test", "test.sh"),
            overlay.path().to_path_buf(),
            mode,
        );

        let error = task
            .run(&ctx)
            .expect_err("a vanished script cannot count as successful or benignly skipped");

        assert!(
            format!("{error:#}").contains("script not found"),
            "{mode:?}, dry_run={dry_run}: {error:#}"
        );
    }
}

#[test]
fn removal_task_propagates_remove_failure() {
    let case = ShellCase::remove([
        check(0),
        (
            Some("--remove"),
            ExecResult::failure("", "remove failed", Some(2)),
        ),
    ]);
    let error = case.run().unwrap_err().to_string();
    assert!(error.contains("removing script"), "{error}");
}

#[test]
fn overlay_script_tasks_have_unique_task_ids() {
    // Multiple OverlayScriptTask instances must produce distinct TaskIds so
    // the parallel scheduler does not report a false "dependency cycle".
    use crate::engine::TaskId;
    use std::collections::HashSet;

    let scripts = [
        script_entry("A", "a.sh"),
        script_entry("B", "b.ps1"),
        script_entry("C", "c.sh"),
    ];
    let tasks = overlay_script_tasks(&scripts, Path::new("/overlay"));
    let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
    let unique: HashSet<TaskId> = ids.iter().cloned().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "all overlay script task IDs must be distinct"
    );
    // Each id must be the structured dynamic variant.
    assert!(
        ids.iter()
            .all(|id| matches!(id, TaskId::NamedDynamic { .. })),
        "overlay script tasks should use TaskId::NamedDynamic, not TaskId::Type"
    );
}

#[test]
fn script_task_run_is_ok_when_check_reports_correct() {
    let case = ShellCase::apply([check(0)]);
    assert!(matches!(case.run().unwrap(), TaskResult::Ok));
}

#[test]
fn script_task_run_reports_changed_when_check_reports_missing() {
    let case = ShellCase::apply([check(1), (None, ExecResult::success("applied\n"))]);
    assert_task_changed(&case.run().unwrap());
}

#[test]
fn script_task_execute_records_changed_status_and_stdout() {
    let case = ShellCase::apply([check(1), (None, ExecResult::success("script applied\n"))]);
    let mut logger = Logger::new_in("install", case.overlay.path());
    logger.set_verbose(false);
    let logger = Arc::new(logger);
    let buffered = Arc::new(BufferedLog::new(Arc::clone(&logger)));
    let ctx = case.ctx.with_log(Arc::<BufferedLog>::clone(&buffered));
    let status = execute(&case.task, &ctx);
    assert_eq!(status, TaskStatus::Changed);
    buffered.flush_and_complete(&case.task.log_key(), case.task.name());

    let entries = logger.task_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, TaskStatus::Changed);
    assert!(
        logger.task_console_output_emitted(),
        "a changed overlay script should produce a durable non-verbose task block"
    );

    let contents =
        std::fs::read_to_string(logger.log_path().expect("overlay script run log")).unwrap();
    assert!(
        contents.contains("script applied"),
        "captured script stdout should remain in the durable run log: {contents}"
    );
}

#[test]
fn script_task_run_uses_dry_run_script_when_context_is_dry_run() {
    let case = ShellCase::apply([
        check(1),
        (Some("--dryrun"), ExecResult::success("would apply\n")),
    ]);
    assert!(matches!(
        case.task.run(&case.ctx.with_dry_run(true)).unwrap(),
        TaskResult::Batch(stats) if stats.changed_count() > 0
    ));
}

#[test]
fn script_task_run_propagates_check_failures() {
    let case = ShellCase::apply([(Some("--check"), ExecResult::failure("", "boom", Some(2)))]);
    let error = case.run().unwrap_err().to_string();
    assert!(
        error.contains("exit 2") && error.contains("boom"),
        "{error}"
    );
}

#[test]
fn script_task_run_fails_when_script_is_missing() {
    let overlay = tempfile::tempdir().expect("create overlay dir");
    let mock = MockExecutor::new();
    let ctx = context_with_executor(overlay.path(), mock);
    let task = OverlayScriptTask::new(
        script_entry("Missing script", "scripts/missing.sh"),
        overlay.path().to_path_buf(),
    );
    let error = task.run(&ctx).unwrap_err().to_string();
    assert!(error.contains("script not found"), "{error}");
}
