#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing
)]
//! Integration tests that exercise task execution against a real filesystem.
//!
//! Unlike the structural tests in `install_command.rs` (which verify task lists,
//! names, and dependency graphs), these tests call `task.run(&ctx)` and
//! `tasks::execute()` to validate that the full config-load → task-execution →
//! filesystem-outcome pipeline works correctly.

mod common;

use dotfiles_cli::tasks;
use dotfiles_cli::tasks::hooks::{InstallGitHooks, UninstallGitHooks};
use dotfiles_cli::tasks::symlinks::InstallSymlinks;
#[cfg(unix)]
use dotfiles_cli::tasks::symlinks::UninstallSymlinks;
use dotfiles_cli::tasks::{Task, TaskResult};

// ===========================================================================
// Symlink task execution
// ===========================================================================

/// Loading symlinks from a TOML fixture and running `InstallSymlinks` must
/// create the expected symlinks in the home directory.
#[cfg(unix)]
#[test]
fn symlinks_install_creates_links_from_config() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source_content("bashrc", "# bash config")
        .build();

    let ec = test.make_context("base");
    let result = InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));

    let link = ec.ctx.home.join(".bashrc");
    assert!(link.symlink_metadata().is_ok(), "symlink should exist");
    let target = std::fs::read_link(&link).unwrap();
    assert_eq!(
        target,
        test.root_path().join("symlinks/bashrc"),
        "symlink should point to source"
    );
}

/// Dry-run mode must report `DryRun` and must not create any symlinks.
#[cfg(unix)]
#[test]
fn symlinks_install_dry_run_creates_no_links() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .build();

    let ec = test.make_dry_run_context("base");
    let result = InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::DryRun));

    let link = ec.ctx.home.join(".bashrc");
    assert!(!link.exists(), "dry-run should not create any symlinks");
}

/// Running `InstallSymlinks` twice on the same repo must succeed both times
/// (idempotency). The second run should find the symlink already correct.
#[cfg(unix)]
#[test]
fn symlinks_install_is_idempotent() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .build();

    let ec = test.make_context("base");

    let first = InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(first, TaskResult::Ok));

    let second = InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(second, TaskResult::Ok));

    // Symlink must still be valid after both runs
    let link = ec.ctx.home.join(".bashrc");
    assert!(link.symlink_metadata().unwrap().is_symlink());
}

/// Installing and then uninstalling symlinks must materialise the file content
/// (copy the source into the target location as a regular file).
#[cfg(unix)]
#[test]
fn symlinks_uninstall_materialises_content() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source_content("bashrc", "# restored content")
        .build();

    let ec = test.make_context("base");

    // Install first
    InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(
        ec.ctx
            .home
            .join(".bashrc")
            .symlink_metadata()
            .unwrap()
            .is_symlink()
    );

    // Uninstall
    let result = UninstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));

    // Target must now be a regular file with the original content
    let meta = std::fs::symlink_metadata(ec.ctx.home.join(".bashrc")).unwrap();
    assert!(
        !meta.is_symlink(),
        "target should be a regular file after uninstall"
    );
    assert_eq!(
        std::fs::read_to_string(ec.ctx.home.join(".bashrc")).unwrap(),
        "# restored content"
    );
}

/// The desktop profile should pick up symlinks from both `[base]` and
/// `[desktop]` sections in the config.
#[cfg(unix)]
#[test]
fn symlinks_install_desktop_profile_includes_both_sections() {
    let test = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            include_str!("fixtures/desktop_profile.toml"),
        )
        .with_symlink_source("bashrc")
        .with_symlink_source("config/Code/User/settings.json")
        .build();

    let ec = test.make_context("desktop");
    let result = InstallSymlinks.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));

    assert!(
        ec.ctx
            .home
            .join(".bashrc")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
        "base-section symlink should be installed"
    );
    assert!(
        ec.ctx
            .home
            .join(".config/Code/User/settings.json")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
        "desktop-section symlink should be installed"
    );
}

// ===========================================================================
// Git hook task execution
// ===========================================================================

/// Running `InstallGitHooks` with hook sources and a `.git/hooks/` dir must
/// install the hooks.
#[test]
fn hooks_install_places_hooks_in_git_dir() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_context("base");
    let task = InstallGitHooks::new();
    let result = task.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));

    let installed = test.root_path().join(".git/hooks/pre-commit");
    assert!(
        installed.exists(),
        "hook should be installed in .git/hooks/"
    );
}

/// Dry-run mode must not install any hooks.
#[test]
fn hooks_install_dry_run_preserves_state() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_dry_run_context("base");
    let task = InstallGitHooks::new();
    let result = task.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::DryRun));

    let installed = test.root_path().join(".git/hooks/pre-commit");
    assert!(!installed.exists(), "dry-run should not install any hooks");
}

/// Running `InstallGitHooks` twice must succeed both times (idempotency).
#[test]
fn hooks_install_is_idempotent() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_context("base");
    let task = InstallGitHooks::new();

    let first = task.run(&ec.ctx).unwrap();
    assert!(matches!(first, TaskResult::Ok));

    let second = task.run(&ec.ctx).unwrap();
    assert!(matches!(second, TaskResult::Ok));

    assert!(test.root_path().join(".git/hooks/pre-commit").exists());
}

/// Installing and then uninstalling hooks must remove the hook file.
#[test]
fn hooks_uninstall_removes_installed_hook() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_context("base");
    InstallGitHooks::new().run(&ec.ctx).unwrap();
    assert!(test.root_path().join(".git/hooks/pre-commit").exists());

    let result = UninstallGitHooks::new().run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
    assert!(
        !test.root_path().join(".git/hooks/pre-commit").exists(),
        "hook should be removed after uninstall"
    );
}

/// Data files with extensions (e.g., `.ini`) must not be installed as hooks.
#[test]
fn hooks_install_skips_data_files_with_extensions() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_hook_source("sensitive-patterns.ini", "[patterns]\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_context("base");
    InstallGitHooks::new().run(&ec.ctx).unwrap();

    assert!(
        test.root_path().join(".git/hooks/pre-commit").exists(),
        "hook without extension should be installed"
    );
    assert!(
        !test
            .root_path()
            .join(".git/hooks/sensitive-patterns.ini")
            .exists(),
        "data file with extension should NOT be installed"
    );
}

// ===========================================================================
// Chmod task execution
// ===========================================================================

/// Applying chmod must set the expected permissions on the target file.
#[cfg(unix)]
#[test]
fn chmod_applies_permissions_from_config() {
    use std::os::unix::fs::PermissionsExt as _;

    let test = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ path = \"ssh/config\", mode = \"600\" }]\n",
        )
        .build();

    let ec = test.make_context("base");

    // Create the target file in the home directory
    let target = ec.ctx.home.join(".ssh/config");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "Host *\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

    let task = dotfiles_cli::tasks::chmod::ApplyFilePermissions;
    let result = task.run(&ec.ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));

    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "permissions should be 0600 after chmod");
}

/// Chmod must be idempotent — running twice should succeed without changes.
#[cfg(unix)]
#[test]
fn chmod_is_idempotent() {
    use std::os::unix::fs::PermissionsExt as _;

    let test = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ path = \"ssh/config\", mode = \"600\" }]\n",
        )
        .build();

    let ec = test.make_context("base");

    let target = ec.ctx.home.join(".ssh/config");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "Host *\n").unwrap();

    let task = dotfiles_cli::tasks::chmod::ApplyFilePermissions;
    task.run(&ec.ctx).unwrap();
    let second = task.run(&ec.ctx).unwrap();
    assert!(matches!(second, TaskResult::Ok));

    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

// ===========================================================================
// tasks::execute() wrapper
// ===========================================================================

/// `tasks::execute()` must record a successful task without failures.
#[cfg(unix)]
#[test]
fn execute_records_no_failures_for_successful_task() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .build();

    let ec = test.make_context("base");
    tasks::execute(&InstallSymlinks, &ec.ctx);

    assert_eq!(
        ec.log.failure_count(),
        0,
        "successful task should record no failures"
    );
}

/// A task that is not applicable (empty config) must be recorded without failure.
#[test]
fn execute_records_not_applicable_when_skipped() {
    let test = common::TestContextBuilder::new().build();
    let ec = test.make_context("base");

    tasks::execute(&InstallSymlinks, &ec.ctx);
    assert_eq!(
        ec.log.failure_count(),
        0,
        "skipped task should not count as a failure"
    );
}

/// Running `execute()` on a task that is not applicable and then a task that
/// succeeds should still report zero failures.
#[cfg(unix)]
#[test]
fn execute_mixed_skip_and_success() {
    let test = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_context("base");

    // Symlinks task has empty config → skipped
    tasks::execute(&InstallSymlinks, &ec.ctx);
    // Hooks task has real hooks → succeeds
    tasks::execute(&InstallGitHooks::new(), &ec.ctx);

    assert_eq!(ec.log.failure_count(), 0);
    assert!(!ec.log.has_failures());
}

// ===========================================================================
// Dry-run pipeline
// ===========================================================================

/// Running filesystem-safe install tasks in dry-run mode against a minimal
/// config must not produce any failures. This validates the config → task →
/// resource pipeline without side effects.
///
/// Tasks that require an executor (packages, shell, git operations) are
/// excluded — they are not filesystem-only and would panic on the stub.
const FILESYSTEM_TASKS: &[&str] = &[
    "Install symlinks",
    "Install git hooks",
    "Apply file permissions",
    "Install Copilot skills",
];

#[test]
fn dry_run_pipeline_produces_no_failures() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .build();

    let ec = test.make_dry_run_context("base");

    for task in tasks::all_install_tasks() {
        if FILESYSTEM_TASKS.contains(&task.name()) && task.should_run(&ec.ctx) {
            tasks::execute(task.as_ref(), &ec.ctx);
        }
    }

    assert!(
        !ec.log.has_failures(),
        "dry-run pipeline should produce no failures"
    );
}
