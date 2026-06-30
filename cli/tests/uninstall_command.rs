#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `uninstall` command.
//!
//! These tests verify the structure and naming of the uninstall task list
//! returned by [`all_uninstall_tasks`].

mod common;

use dotfiles_cli::testing as test_api;
use std::collections::HashSet;

use test_api::platform::{Os, Platform};
use test_api::tasks;
use test_api::tasks::TaskId;

// ---------------------------------------------------------------------------
// Snapshot: full uninstall task list
// ---------------------------------------------------------------------------

/// Snapshot of all uninstall task names in their declared order.
///
/// Any addition, removal, or rename of an uninstall task will cause this test
/// to fail, prompting a deliberate snapshot update.
#[test]
fn uninstall_task_names() {
    let all_tasks = tasks::all_uninstall_tasks();
    let task_names: Vec<&str> = all_tasks.iter().map(|t| t.name()).collect();
    insta::assert_snapshot!("uninstall_task_names", task_names.join("\n"));
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

/// The uninstall task list must contain the expected number of tasks.
#[test]
fn uninstall_task_count() {
    assert_eq!(tasks::all_uninstall_tasks().len(), 3);
}

/// Every uninstall task name must be non-empty.
#[test]
fn uninstall_task_names_are_non_empty() {
    for task in tasks::all_uninstall_tasks() {
        assert!(!task.name().is_empty(), "uninstall task has an empty name");
    }
}

/// No two uninstall tasks may share the same name.
#[test]
fn uninstall_task_names_are_unique() {
    let tasks = tasks::all_uninstall_tasks();
    let mut seen: HashSet<&str> = HashSet::new();
    for task in &tasks {
        assert!(
            seen.insert(task.name()),
            "duplicate uninstall task name: '{}'",
            task.name()
        );
    }
}

/// No two uninstall tasks may share the same [`TaskId`].
#[test]
fn uninstall_task_type_ids_are_unique() {
    let tasks = tasks::all_uninstall_tasks();
    let ids: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "uninstall task list contains duplicate TaskIds"
    );
}

/// Every dependency declared by an uninstall task must be satisfied by another
/// task in the same list.
#[test]
fn uninstall_task_dependencies_are_resolvable() {
    let tasks = tasks::all_uninstall_tasks();
    let present: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
    for task in &tasks {
        for dep in task.dependencies() {
            assert!(
                present.contains(dep),
                "uninstall task '{}' declares a dependency not in the uninstall task list",
                task.name()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

/// "Remove symlinks" must be present in the uninstall task list.
#[test]
fn uninstall_task_list_contains_remove_symlinks() {
    let tasks = tasks::all_uninstall_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Remove symlinks"),
        "expected 'Remove symlinks' in uninstall task list, got: {names:?}"
    );
}

/// "Remove Git hooks" must be present in the uninstall task list.
#[test]
fn uninstall_task_list_contains_remove_git_hooks() {
    let tasks = tasks::all_uninstall_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Remove Git hooks"),
        "expected 'Remove Git hooks' in uninstall task list, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any uninstall task when given a minimal config.
#[test]
fn uninstall_tasks_should_run_does_not_panic_with_minimal_config() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();
    let config = ctx.load_config("base");

    let platform = Platform::detect();
    let executor: Arc<dyn test_api::exec::Executor> = Arc::new(test_api::exec::SystemExecutor);
    let log: Arc<test_api::logging::Logger> =
        Arc::new(test_api::logging::Logger::new("test-uninstall"));

    let task_ctx = tasks::Context::new(
        Arc::new(std::sync::RwLock::new(Arc::new(config))),
        platform,
        Arc::clone(&log) as Arc<dyn test_api::logging::Log>,
        executor,
        tasks::ContextOpts {
            dry_run: true,
            parallel: false,
            advance_versions: false,
            is_ci: None,
        },
    )
    .expect("create context");

    let tasks = tasks::all_uninstall_tasks();
    for task in &tasks {
        let _ = task.should_run(&task_ctx);
    }
}

// ---------------------------------------------------------------------------
// Dependency graph: no cycles
// ---------------------------------------------------------------------------

/// The uninstall task dependency graph must not contain any cycles.
#[test]
fn uninstall_tasks_form_acyclic_dependency_graph() {
    use test_api::engine::graph::validate;

    let tasks = tasks::all_uninstall_tasks();
    let task_refs: Vec<&dyn tasks::Task> = tasks.iter().map(Box::as_ref).collect();
    assert_eq!(
        validate(&task_refs),
        Ok(()),
        "uninstall task dependency graph is not a valid DAG"
    );
}

// ---------------------------------------------------------------------------
// Idempotency: uninstall → uninstall is a no-op
// ---------------------------------------------------------------------------

/// Running `UninstallSymlinks` twice must succeed on both calls.
///
/// After the first uninstall the symlink is materialised to a regular file.
/// The second call must return `TaskResult::Ok` without panicking or erroring
/// because the target is no longer a symlink (`process_resources_remove`
/// silently skips resources that are not in the `Correct` state).
#[cfg(unix)]
#[test]
fn uninstall_symlinks_is_idempotent() {
    use std::sync::Arc;

    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
        .with_symlink_source("bashrc")
        .build();

    let home_dir = tempfile::tempdir().expect("create temp home dir");

    let platform = Platform::detect();
    let executor: Arc<dyn test_api::exec::Executor> = Arc::new(test_api::exec::SystemExecutor);
    let log: Arc<test_api::logging::Logger> =
        Arc::new(test_api::logging::Logger::new("test-uninstall-idempotent"));

    let config = ctx.load_config("base");
    let task_ctx = tasks::Context::from_raw(
        Arc::new(std::sync::RwLock::new(Arc::new(config))),
        platform,
        Arc::clone(&log) as Arc<dyn test_api::logging::Log>,
        executor,
        home_dir.path().to_path_buf(),
        tasks::ContextOpts {
            dry_run: false,
            parallel: false,
            advance_versions: false,
            is_ci: Some(false),
        },
    );

    // Install the symlink first so there is something to uninstall.
    let install_result = tasks::files::symlinks::InstallSymlinks
        .run(&task_ctx)
        .expect("install run");
    assert!(
        matches!(install_result, tasks::TaskResult::Ok),
        "install run should succeed"
    );

    // First uninstall: symlink must be materialised to a regular file.
    let result1 = tasks::files::symlinks::UninstallSymlinks
        .run(&task_ctx)
        .expect("first uninstall run");
    assert!(
        matches!(result1, tasks::TaskResult::Ok),
        "first uninstall run should succeed"
    );

    let target = home_dir.path().join(".bashrc");
    let meta = std::fs::symlink_metadata(&target).expect("target should exist after uninstall");
    assert!(
        !meta.is_symlink(),
        "target should be materialised to a regular file after uninstall"
    );

    // Second uninstall: must succeed (idempotency — target is no longer a symlink).
    let result2 = tasks::files::symlinks::UninstallSymlinks
        .run(&task_ctx)
        .expect("second uninstall run");
    assert!(
        matches!(result2, tasks::TaskResult::Ok),
        "second uninstall run should succeed (idempotency guarantee)"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list with a Windows platform
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any uninstall task when given a Windows platform.
#[test]
fn uninstall_tasks_should_run_with_windows_platform() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);

    let executor: Arc<dyn test_api::exec::Executor> = Arc::new(test_api::exec::SystemExecutor);
    let log: Arc<test_api::logging::Logger> =
        Arc::new(test_api::logging::Logger::new("test-uninstall-windows"));

    let task_ctx = tasks::Context::new(
        Arc::new(std::sync::RwLock::new(Arc::new(config))),
        platform,
        Arc::clone(&log) as Arc<dyn test_api::logging::Log>,
        executor,
        tasks::ContextOpts {
            dry_run: true,
            parallel: false,
            advance_versions: false,
            is_ci: None,
        },
    )
    .expect("create context");

    let all_tasks = tasks::all_uninstall_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&task_ctx);
    }
}
