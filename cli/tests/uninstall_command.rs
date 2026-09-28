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

#[cfg(unix)]
#[test]
fn uninstall_removes_active_overlay_script_state_by_default() {
    let repo = common::IntegrationTestContext::new();
    let overlay = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(overlay.path().join("conf")).unwrap();
    std::fs::create_dir_all(overlay.path().join("scripts")).unwrap();
    std::fs::write(
        overlay.path().join("conf/scripts.toml"),
        "[base]\nscripts = [{ name = 'Private tools', path = 'scripts/tools.sh' }]\n",
    )
    .unwrap();
    std::fs::write(overlay.path().join("state"), "managed\n").unwrap();
    std::fs::write(
        overlay.path().join("scripts/tools.sh"),
        "#!/bin/sh\ncase \"$1\" in\n  --check) test -f state ;;\n  --remove) rm state ;;\n  --dryrun) touch unexpected-dryrun ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();

    let run = |dry_run| {
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            Some(overlay.path()),
            "uninstall",
            "script-private-tools",
        );
        if dry_run {
            command.arg("--dry-run");
        }
        command.output().unwrap()
    };

    let preview = run(true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(overlay.path().join("state").exists());
    assert!(!overlay.path().join("unexpected-dryrun").exists());

    let removed = run(false);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!overlay.path().join("state").exists());

    let repeated = run(false);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
}
