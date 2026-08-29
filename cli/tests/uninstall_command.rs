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

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;

/// Build an uninstall task list backed by a store loaded from a minimal repo.
fn uninstall_tasks() -> Vec<Box<dyn tasks::Task>> {
    uninstall_tasks_for_platform(Platform::detect())
}

fn uninstall_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_uninstall_tasks(&store)
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

#[test]
fn uninstall_task_catalog_satisfies_structural_contract() {
    let tasks = uninstall_tasks();
    common::assert_task_catalog_contract("uninstall", &tasks);
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

#[test]
fn uninstall_task_catalog_contains_required_tasks() {
    let tasks = uninstall_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["symlinks", "git-hooks"] {
        assert!(
            selectors.contains(&required),
            "uninstall task catalog is missing required selector '{required}'"
        );
    }
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn uninstall_tasks_assess_on_linux_and_windows() {
    let platforms = [
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
        Platform {
            os: Os::Windows,
            is_arch: false,
            is_wsl: false,
        },
    ];

    for platform in platforms {
        let ctx = common::TestContextBuilder::new().build();
        let ec = ctx.make_system_context(
            "base",
            platform,
            tasks::ContextOpts {
                dry_run: true,
                parallel: false,
                is_ci: None,
            },
        );

        for task in tasks::all_uninstall_tasks(&ec.store) {
            let _ = task.should_run(&ec.ctx);
        }
    }
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
    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
        .with_symlink_source("bashrc")
        .build();

    let ec = ctx.make_context("base");

    // Install the symlink first so there is something to uninstall.
    let install_result = tasks::files::symlinks::InstallSymlinks::new(ec.store.symlinks.clone())
        .run(&ec.ctx)
        .expect("install run");
    assert!(
        matches!(
            install_result,
            tasks::TaskResult::Batch(ref stats) if stats.changed_count() > 0
        ),
        "install run should succeed"
    );

    // First uninstall: symlink must be materialised to a regular file.
    let result1 = tasks::files::symlinks::UninstallSymlinks::new(ec.store.symlinks.clone())
        .run(&ec.ctx)
        .expect("first uninstall run");
    assert!(
        matches!(result1, tasks::TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "first uninstall run should succeed"
    );

    let target = ec.ctx.home().join(".bashrc");
    let meta = std::fs::symlink_metadata(&target).expect("target should exist after uninstall");
    assert!(
        !meta.is_symlink(),
        "target should be materialised to a regular file after uninstall"
    );

    // Second uninstall: must succeed (idempotency — target is no longer a symlink).
    let result2 = tasks::files::symlinks::UninstallSymlinks::new(ec.store.symlinks)
        .run(&ec.ctx)
        .expect("second uninstall run");
    assert!(
        matches!(
            result2,
            tasks::TaskResult::Batch(ref stats)
                if stats.changed_count() == 0 && stats.failed_count() == 0
        ),
        "second uninstall run should succeed (idempotency guarantee)"
    );
}
