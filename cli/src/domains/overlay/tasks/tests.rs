//! Unit tests for overlay script tasks.

use super::*;
use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::engine::{Context, Task, TaskResult};
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{empty_config, make_context, make_linux_context};
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
fn snapshot_report_should_run_false_without_overlay() {
    let config = empty_config(PathBuf::from("/tmp"));
    let ctx = make_linux_context(config);
    assert!(
        !ReportOverlayScriptSnapshot::new(crate::infra::ConfigHandle::new(vec![])).should_run(&ctx)
    );
}

#[test]
fn snapshot_report_should_run_true_with_overlay() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.overlay = Some(PathBuf::from("/overlay"));
    let ctx = make_linux_context(config);
    assert!(
        ReportOverlayScriptSnapshot::new(crate::infra::ConfigHandle::new(vec![])).should_run(&ctx)
    );
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
    use crate::engine::TaskId;
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
