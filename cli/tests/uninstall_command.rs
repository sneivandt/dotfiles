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
fn uninstall_catalog_applicability_tracks_managed_inputs_on_both_platforms() {
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
        for configured in [false, true] {
            let mut builder = common::TestContextBuilder::new();
            if configured {
                builder = builder
                    .with_config_file("symlinks.toml", "[base]\nsymlinks = ['example']\n")
                    .with_symlink_source("example")
                    .with_git_hooks_dir();
            }
            let ctx = builder.build();
            let ec = ctx.make_context_with_executor(
                "base",
                platform,
                tasks::ContextOpts {
                    dry_run: true,
                    parallel: false,
                    is_ci: Some(false),
                },
                std::sync::Arc::new(common::StubExecutor),
            );
            let mut applicable: Vec<_> = tasks::all_uninstall_tasks(&ec.store)
                .into_iter()
                .filter(|task| task.should_run(&ec.ctx))
                .map(|task| task.selector().to_string())
                .collect();
            applicable.sort_unstable();
            let expected = if configured {
                vec!["git-hooks", "launcher", "symlinks"]
            } else {
                vec!["launcher"]
            };
            assert_eq!(
                applicable, expected,
                "{platform:?}, configured={configured}"
            );
        }
    }
}

#[test]
fn invalid_uninstall_selection_fails_without_removing_managed_hooks() {
    for (selector, skip, diagnostic) in [
        ("unknown-task", None, "--only did not match a task selector"),
        ("git-hooks", Some("git-hooks"), "selected no tasks"),
    ] {
        let repo = common::TestContextBuilder::new()
            .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
            .with_git_hooks_dir()
            .build();
        let hook = repo
            .root_path()
            .join(".git")
            .join("hooks")
            .join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "uninstall", selector);
        if let Some(skip) = skip {
            command.args(["--skip", skip]);
        }
        let output = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "{selector}: {text}");
        assert!(text.contains(diagnostic), "{selector}: {text}");
        assert_eq!(
            std::fs::read_to_string(hook).unwrap(),
            "#!/bin/sh\nexit 0\n",
            "{selector}: invalid selection must not remove hooks"
        );
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
