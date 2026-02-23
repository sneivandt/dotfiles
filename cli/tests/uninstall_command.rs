#![allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
//! Integration tests for the `uninstall` command.
//!
//! These tests verify the structure and naming of the uninstall task list
//! returned by [`all_uninstall_tasks`].

mod common;

use std::any::TypeId;
use std::collections::HashSet;

use dotfiles_cli::tasks;

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
    assert_eq!(tasks::all_uninstall_tasks().len(), 2);
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

/// No two uninstall tasks may share the same [`TypeId`].
#[test]
fn uninstall_task_type_ids_are_unique() {
    let tasks = tasks::all_uninstall_tasks();
    let ids: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "uninstall task list contains duplicate TypeIds"
    );
}

/// Every dependency declared by an uninstall task must be satisfied by another
/// task in the same list.
#[test]
fn uninstall_task_dependencies_are_resolvable() {
    let tasks = tasks::all_uninstall_tasks();
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
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

/// "Remove git hooks" must be present in the uninstall task list.
#[test]
fn uninstall_task_list_contains_remove_git_hooks() {
    let tasks = tasks::all_uninstall_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Remove git hooks"),
        "expected 'Remove git hooks' in uninstall task list, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any uninstall task when given an empty config.
#[test]
fn uninstall_tasks_should_run_does_not_panic_with_empty_config() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();
    let config = ctx.load_config("base");

    let platform = dotfiles_cli::platform::Platform::detect();
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new(false, "test-uninstall"));

    let task_ctx = dotfiles_cli::tasks::Context::new(
        Arc::new(std::sync::RwLock::new(config)),
        Arc::new(platform),
        Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        true, // dry_run
        executor,
        false, // parallel
    )
    .expect("create context");

    let tasks = tasks::all_uninstall_tasks();
    for task in &tasks {
        let _ = task.should_run(&task_ctx);
    }
}
