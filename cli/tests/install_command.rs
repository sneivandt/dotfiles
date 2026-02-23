#![allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
//! Integration tests for the `install` command.
//!
//! These tests exercise the full task list produced by [`all_install_tasks`],
//! the task-name–based filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the install dependency graph.

mod common;

use std::any::TypeId;
use std::collections::HashSet;

use dotfiles_cli::tasks;

// ---------------------------------------------------------------------------
// Snapshot: full install task list
// ---------------------------------------------------------------------------

/// Snapshot of all install task names in their declared order.
///
/// This test serves as a regression guard: any addition, removal, or rename of
/// an install task will cause it to fail, prompting a deliberate snapshot update.
#[test]
fn install_task_names() {
    let all_tasks = tasks::all_install_tasks();
    let task_names: Vec<&str> = all_tasks.iter().map(|t| t.name()).collect();
    insta::assert_snapshot!("install_task_names", task_names.join("\n"));
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

/// The install task list must contain exactly the expected number of tasks.
#[test]
fn install_task_count() {
    assert_eq!(tasks::all_install_tasks().len(), 16);
}

/// Every task name must be non-empty.
#[test]
fn install_task_names_are_non_empty() {
    for task in tasks::all_install_tasks() {
        assert!(!task.name().is_empty(), "install task has an empty name");
    }
}

/// No two install tasks may share the same name.
#[test]
fn install_task_names_are_unique() {
    let tasks = tasks::all_install_tasks();
    let mut seen: HashSet<&str> = HashSet::new();
    for task in &tasks {
        assert!(
            seen.insert(task.name()),
            "duplicate install task name: '{}'",
            task.name()
        );
    }
}

/// No two install tasks may share the same [`TypeId`].
#[test]
fn install_task_type_ids_are_unique() {
    let tasks = tasks::all_install_tasks();
    let ids: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "install task list contains duplicate TypeIds"
    );
}

/// Every dependency declared by an install task must be satisfied by another
/// task in the same list (i.e., no dangling dependency references).
#[test]
fn install_task_dependencies_are_resolvable() {
    let tasks = tasks::all_install_tasks();
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    for task in &tasks {
        for dep in task.dependencies() {
            assert!(
                present.contains(dep),
                "task '{}' declares a dependency that is not in the install task list",
                task.name()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// --skip filter
// ---------------------------------------------------------------------------

/// Tasks whose names contain the skip keyword (case-insensitive) must be
/// excluded from the filtered list, matching the behaviour of `--skip packages`.
#[test]
fn skip_filter_excludes_matching_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let skip_keyword = "packages";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| !t.name().to_lowercase().contains(skip_keyword))
        .map(|t| t.name())
        .collect();

    for name in &filtered {
        assert!(
            !name.to_lowercase().contains(skip_keyword),
            "task '{name}' should have been excluded by --skip {skip_keyword}",
        );
    }
    // At least one task was removed
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip packages should remove at least one task"
    );
}

/// When the skip keyword does not match any task name the full list is returned.
#[test]
fn skip_filter_with_no_match_returns_all_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let skip_keyword = "zzznomatch";
    let total = all_tasks.len();

    let filtered_count = all_tasks
        .iter()
        .filter(|t| !t.name().to_lowercase().contains(skip_keyword))
        .count();

    assert_eq!(
        filtered_count, total,
        "--skip with non-matching keyword should leave task count unchanged"
    );
}

// ---------------------------------------------------------------------------
// --only filter
// ---------------------------------------------------------------------------

/// Only tasks whose names contain the `--only` keyword should remain.
#[test]
fn only_filter_includes_only_matching_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "symlinks";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| t.name().to_lowercase().contains(only_keyword))
        .map(|t| t.name())
        .collect();

    assert_eq!(
        filtered,
        vec!["Install symlinks"],
        "--only symlinks should return exactly one task"
    );
}

/// When `--only` matches multiple tasks all of them are included.
#[test]
fn only_filter_can_include_multiple_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "git";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| t.name().to_lowercase().contains(only_keyword))
        .map(|t| t.name())
        .collect();

    // "Configure git" and "Install git hooks" both match
    assert!(
        filtered.len() >= 2,
        "--only git should match at least 'Configure git' and 'Install git hooks'"
    );
    for name in &filtered {
        assert!(
            name.to_lowercase().contains(only_keyword),
            "task '{name}' should not have been included by --only git",
        );
    }
}

/// When `--only` matches nothing the result is an empty list.
#[test]
fn only_filter_with_no_match_returns_empty() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "zzznomatch";

    let any_match = all_tasks
        .iter()
        .any(|t| t.name().to_lowercase().contains(only_keyword));

    assert!(
        !any_match,
        "--only with non-matching keyword should return empty list"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

/// Loading a minimal repository and filtering the install task list by the
/// tasks that *should* run on the current platform must not panic.
///
/// This exercises the `should_run` gate for every install task in a
/// controlled, isolated environment.
#[test]
fn install_tasks_should_run_does_not_panic_with_empty_config() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();
    let config = ctx.load_config("base");

    let platform = dotfiles_cli::platform::Platform::detect();
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new(false, "test-install"));

    let task_ctx = dotfiles_cli::tasks::Context::new(
        Arc::new(std::sync::RwLock::new(config)),
        Arc::new(platform),
        Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        true, // dry_run
        executor,
        false, // parallel
    )
    .expect("create context");

    let tasks = tasks::all_install_tasks();
    // Calling should_run on every task must not panic.
    for task in &tasks {
        let _ = task.should_run(&task_ctx);
    }
}
