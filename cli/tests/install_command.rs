#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `install` command.
//!
//! These tests exercise the full task list produced by [`all_install_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the install dependency graph.

mod common;

use dotfiles_cli::testing as test_api;
use std::collections::HashSet;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;
use test_api::tasks::TaskId;
use test_api::tasks::filter::task_matches_filter;

/// Build an install task list backed by a store loaded from a minimal repo.
fn install_tasks() -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config("base"));
    tasks::all_install_tasks(store)
}

// ---------------------------------------------------------------------------
// Snapshot: full install task list
// ---------------------------------------------------------------------------

/// Snapshot of all install task names in their declared order.
///
/// This test serves as a regression guard: any addition, removal, or rename of
/// an install task will cause it to fail, prompting a deliberate snapshot update.
#[test]
fn install_task_names() {
    let all_tasks = install_tasks();
    let task_names: Vec<&str> = all_tasks.iter().map(|t| t.name()).collect();
    insta::assert_snapshot!("install_task_names", task_names.join("\n"));
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

/// The install task list must contain exactly the expected number of tasks.
#[test]
fn install_task_count() {
    assert_eq!(install_tasks().len(), 24);
}

/// Every task name must be non-empty.
#[test]
fn install_task_names_are_non_empty() {
    for task in install_tasks() {
        assert!(!task.name().is_empty(), "install task has an empty name");
    }
}

/// No two install tasks may share the same name.
#[test]
fn install_task_names_are_unique() {
    let tasks = install_tasks();
    let mut seen: HashSet<&str> = HashSet::new();
    for task in &tasks {
        assert!(
            seen.insert(task.name()),
            "duplicate install task name: '{}'",
            task.name()
        );
    }
}

/// No two install tasks may share the same [`TaskId`].
#[test]
fn install_task_type_ids_are_unique() {
    let tasks = install_tasks();
    let ids: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "install task list contains duplicate TaskIds"
    );
}

/// Every dependency declared by an install task must be satisfied by another
/// task in the same list (i.e., no dangling dependency references).
#[test]
fn install_task_dependencies_are_resolvable() {
    let tasks = install_tasks();
    let present: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
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

/// Tasks matching the skip selector must be excluded from the filtered list.
#[test]
fn skip_filter_excludes_matching_tasks() {
    let all_tasks = install_tasks();
    let skip_keyword = "packages";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| !task_matches_filter(t.name(), skip_keyword))
        .map(|t| t.name())
        .collect();

    for name in &filtered {
        assert!(
            !task_matches_filter(name, skip_keyword),
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
    let all_tasks = install_tasks();
    let skip_keyword = "zzznomatch";
    let total = all_tasks.len();

    let filtered_count = all_tasks
        .iter()
        .filter(|t| !task_matches_filter(t.name(), skip_keyword))
        .count();

    assert_eq!(
        filtered_count, total,
        "--skip with non-matching keyword should leave task count unchanged"
    );
}

// ---------------------------------------------------------------------------
// --only filter
// ---------------------------------------------------------------------------

/// Only tasks matching the `--only` selector should remain.
#[test]
fn only_filter_includes_only_matching_tasks() {
    let all_tasks = install_tasks();
    let only_keyword = "symlinks";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| task_matches_filter(t.name(), only_keyword))
        .map(|t| t.name())
        .collect();

    assert_eq!(
        filtered,
        vec!["Install symlinks"],
        "--only symlinks should return exactly one task"
    );
}

/// Canonical selectors disambiguate similar task names.
#[test]
fn only_filter_disambiguates_update_tasks() {
    let all_tasks = install_tasks();
    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| task_matches_filter(t.name(), "repository"))
        .map(|t| t.name())
        .collect();

    assert_eq!(filtered, vec!["Update repository"]);

    let unmatched = all_tasks
        .iter()
        .any(|t| task_matches_filter(t.name(), "update"));

    assert!(
        !unmatched,
        "ambiguous selectors like 'update' should not match any task"
    );
}

/// Canonical selector leading tokens should match non-generic task names.
#[test]
fn only_filter_matches_reload_task_by_keyword() {
    let all_tasks = install_tasks();
    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| task_matches_filter(t.name(), "reload"))
        .map(|t| t.name())
        .collect();

    assert_eq!(filtered, vec!["Reload configuration"]);
}

/// When `--only` matches nothing the result is an empty list.
#[test]
fn only_filter_with_no_match_returns_empty() {
    let all_tasks = install_tasks();
    let only_keyword = "zzznomatch";

    let any_match = all_tasks
        .iter()
        .any(|t| task_matches_filter(t.name(), only_keyword));

    assert!(
        !any_match,
        "--only with non-matching keyword should return empty list"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when given a minimal config.
#[test]
fn install_tasks_should_run_does_not_panic_with_minimal_config() {
    let ctx = common::TestContextBuilder::new().build();
    let ec = ctx.make_system_context(
        "base",
        Platform::detect(),
        tasks::ContextOpts {
            dry_run: true,
            parallel: false,
            advance_versions: false,
            is_ci: None,
        },
    );

    let tasks = install_tasks();
    // Calling should_run on every task must not panic.
    for task in &tasks {
        let _ = task.should_run(&ec.ctx);
    }
}

// ---------------------------------------------------------------------------
// Dependency graph: no cycles
// ---------------------------------------------------------------------------

/// The install task dependency graph must not contain any cycles.
///
/// A cyclic dependency would cause the parallel scheduler to deadlock.  This
/// test validates the real task set as a regression guard independent of the
/// scheduler unit tests.
#[test]
fn install_tasks_form_acyclic_dependency_graph() {
    use test_api::engine::graph::validate;

    let tasks = install_tasks();
    let task_refs: Vec<&dyn tasks::Task> = tasks.iter().map(Box::as_ref).collect();
    assert_eq!(
        validate(&task_refs),
        Ok(()),
        "install task dependency graph is not a valid DAG"
    );
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

/// The install task list must contain "Install symlinks".
#[test]
fn install_task_list_contains_install_symlinks() {
    let tasks = install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Install symlinks"),
        "expected 'Install symlinks' in install task list, got: {names:?}"
    );
}

/// The install task list must contain "Install git hooks".
#[test]
fn install_task_list_contains_install_git_hooks() {
    let tasks = install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Install Git hooks"),
        "expected 'Install Git hooks' in install task list, got: {names:?}"
    );
}

/// The install task list must contain "Configure Git".
#[test]
fn install_task_list_contains_configure_git() {
    let tasks = install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Configure Git"),
        "expected 'Configure Git' in install task list, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list with a Windows platform
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when given a Windows platform.
///
/// This exercises the platform-guarding logic in tasks like `ConfigureSystemd`,
/// `ApplyRegistry`, and `ApplyFilePermissions` without needing a real Windows OS.
#[test]
fn install_tasks_should_run_with_windows_platform() {
    let ctx = common::TestContextBuilder::new().build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let ec = ctx.make_system_context(
        "base",
        platform,
        tasks::ContextOpts {
            dry_run: true,
            parallel: false,
            advance_versions: false,
            is_ci: None,
        },
    );

    let all_tasks = install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&ec.ctx);
    }
}

// ---------------------------------------------------------------------------
// --skip filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple keywords are provided, tasks matching any one of them must
/// be excluded.
#[test]
fn skip_with_multiple_keywords_excludes_all_matching() {
    let all_tasks = install_tasks();
    let skip_keywords = ["packages", "registry"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            !skip_keywords
                .iter()
                .any(|kw| task_matches_filter(t.name(), kw))
        })
        .map(|t| t.name())
        .collect();

    for name in &filtered {
        for kw in &skip_keywords {
            assert!(
                !task_matches_filter(name, kw),
                "task '{name}' should have been excluded by --skip {kw}",
            );
        }
    }
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip with multiple keywords should remove at least one task"
    );
}

// ---------------------------------------------------------------------------
// --only filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple selectors are provided, tasks matching any one of them must
/// all be included (union, not intersection).
#[test]
fn only_with_multiple_keywords_includes_all_matching() {
    let all_tasks = install_tasks();
    let only_keywords = ["symlinks", "git-hooks"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            only_keywords
                .iter()
                .any(|kw| task_matches_filter(t.name(), kw))
        })
        .map(|t| t.name())
        .collect();

    assert!(filtered.contains(&"Install symlinks"));
    assert!(filtered.contains(&"Install Git hooks"));

    for name in &filtered {
        assert!(
            only_keywords.iter().any(|kw| task_matches_filter(name, kw)),
            "task '{name}' should not have been included"
        );
    }
}

// ---------------------------------------------------------------------------
// Idempotency: install → install is a no-op
// ---------------------------------------------------------------------------

/// Running `InstallSymlinks` twice must produce zero changes on the second run.
///
/// This test exercises the core idempotency guarantee: after a successful
/// install every resource is already in the desired state, so a second install
/// run must not change anything.
///
/// The verification relies on [`ResourceState`]: if every resource reports
/// `Correct` before the second `run()` call, then `process_resources` can only
/// count items as `already_ok` — making `changed == 0` a logical necessity.
/// Checking `Correct` after the second run confirms no resources were broken.
#[cfg(unix)]
#[test]
fn install_symlinks_is_idempotent() {
    use std::sync::Arc;

    use test_api::resources::IntrinsicState;
    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
        .with_symlink_source("bashrc")
        .build();

    let ec = ctx.make_system_context(
        "base",
        Platform::detect(),
        tasks::ContextOpts {
            dry_run: false,
            parallel: false,
            advance_versions: false,
            is_ci: Some(false),
        },
    );

    let task = tasks::files::symlinks::InstallSymlinks::new(ec.store.symlinks.clone());

    // First run: must succeed and create the symlink.
    let result1 = task.run(&ec.ctx).expect("first install run");
    assert!(
        matches!(result1, tasks::TaskResult::OkWithMessage(_)),
        "first install run should succeed"
    );

    // Build the resource to inspect state directly.
    let source = ctx.root_path().join("symlinks").join("bashrc");
    let target = ec.ctx.home().join(".bashrc");
    let resource = test_api::resources::symlink::SymlinkResource::new(
        source,
        target,
        Arc::new(test_api::exec::SystemExecutor),
    );

    // After the first run every resource must be Correct.  This is the
    // precondition that proves the second run will make zero changes.
    assert_eq!(
        resource
            .current_state()
            .expect("check state after first run"),
        test_api::resources::ResourceState::Correct,
        "symlink must be Correct after first install"
    );

    // Second run: must succeed without changing anything.
    let result2 = task.run(&ec.ctx).expect("second install run");
    assert!(
        matches!(result2, tasks::TaskResult::Ok),
        "second install run should succeed"
    );

    // State must still be Correct, confirming zero changes in the second run.
    assert_eq!(
        resource
            .current_state()
            .expect("check state after second run"),
        test_api::resources::ResourceState::Correct,
        "symlink must still be Correct after second install (idempotency guarantee)"
    );
}

// ---------------------------------------------------------------------------
// ApplyFilePermissions: real filesystem chmod
// ---------------------------------------------------------------------------

/// `ApplyFilePermissions.run()` must set the declared mode on an existing file.
///
/// Creates `$HOME/.ssh/config` with permissions `0o644`, then runs the task
/// and asserts that the permissions are updated to `0o600`.
#[cfg(unix)]
#[test]
fn apply_file_permissions_run_sets_mode_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let ec = ctx.make_system_context(
        "base",
        platform,
        tasks::ContextOpts {
            dry_run: false,
            parallel: false,
            advance_versions: false,
            is_ci: Some(false),
        },
    );

    // Create $HOME/.ssh/config with mode 0o644.
    let ssh_dir = ec.ctx.home().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).expect("create .ssh dir");
    let ssh_config = ssh_dir.join("config");
    std::fs::write(&ssh_config, "").expect("create ssh config");
    std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644))
        .expect("set initial permissions");

    let result = tasks::files::chmod::ApplyFilePermissions::new(ec.store.chmod.clone())
        .run(&ec.ctx)
        .expect("apply file permissions run");
    assert!(
        matches!(result, tasks::TaskResult::OkWithMessage(_)),
        "apply file permissions should succeed"
    );

    let perms = std::fs::metadata(&ssh_config)
        .expect("read file metadata")
        .permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "file permissions should be 0o600 after applying chmod"
    );
}

// ---------------------------------------------------------------------------
// install::run: full dry-run pipeline
// ---------------------------------------------------------------------------

/// Calling `commands::install::run` with `dry_run: true` must return `Ok(())`
/// without making any filesystem changes.
#[test]
fn install_run_dry_run_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec![], false);
    assert!(
        result.is_ok(),
        "dry-run install should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--only symlinks` in dry-run mode must return
/// `Ok(())` and execute only matching tasks.
#[test]
fn install_run_dry_run_with_only_filter_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec!["symlinks".to_string()], false);
    assert!(
        result.is_ok(),
        "dry-run install with --only symlinks should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--skip packages` in dry-run mode must return
/// `Ok(())` and skip matching tasks.
#[test]
fn install_run_dry_run_with_skip_filter_returns_ok() {
    let result = common::run_install_dry_run(vec!["packages".to_string()], vec![], false);
    assert!(
        result.is_ok(),
        "dry-run install with --skip packages should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--only` matching no task name must return
/// `Ok(())` (empty task list is not an error).
#[test]
fn install_run_dry_run_with_only_no_match_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec!["zzznomatch".to_string()], false);
    assert!(
        result.is_ok(),
        "dry-run install with --only no-match should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--only symlinks` in parallel dry-run mode
/// must return `Ok(())`.
#[test]
fn install_run_dry_run_with_only_filter_parallel_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec!["symlinks".to_string()], true);
    assert!(
        result.is_ok(),
        "parallel dry-run with --only symlinks should return Ok: {result:?}"
    );
}

/// Calling `install::run` with both `--skip` and `--only` simultaneously:
/// a task must satisfy `--only` and must not match `--skip`.
#[test]
fn install_run_dry_run_with_skip_and_only_together() {
    // Matching tasks are still excluded when they also match --skip.
    let result = common::run_install_dry_run(
        vec!["symlinks".to_string()],
        vec!["symlinks".to_string()],
        false,
    );
    assert!(
        result.is_ok(),
        "dry-run with --skip and --only should return Ok: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Parallel execution: should_run with parallel enabled
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when `parallel` is `true`.
///
/// This exercises the scheduler path that dispatches resources to Rayon
/// without needing a real system.
#[test]
fn install_tasks_should_run_with_parallel_enabled() {
    let ctx = common::TestContextBuilder::new().build();
    let ec = ctx.make_system_context(
        "base",
        Platform::detect(),
        tasks::ContextOpts {
            dry_run: true,
            parallel: true,
            advance_versions: false,
            is_ci: Some(false),
        },
    );

    let all_tasks = install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&ec.ctx);
    }
}
