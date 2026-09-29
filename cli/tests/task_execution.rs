#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests that exercise task execution against a real filesystem.
//!
//! Unlike the structural tests in `install_command.rs` (which verify task lists,
//! names, and dependency graphs), these tests call `task.run(&ctx)` and
//! `tasks::execute()` to validate that the full config-load → task-execution →
//! filesystem-outcome pipeline works correctly.

mod common;

use dotfiles_cli::testing as test_api;
#[cfg(unix)]
use test_api::tasks::Task;
#[cfg(unix)]
use test_api::tasks::files::chmod::ApplyFilePermissions;
#[cfg(unix)]
use test_api::tasks::files::symlinks::InstallSymlinks;
#[cfg(unix)]
use test_api::tasks::files::symlinks::UninstallSymlinks;
use test_api::tasks::{self, TaskResult};
use test_api::{
    engine::{ProcessOpts, process_resources},
    resources::git_config::GitConfigResource,
};

const fn batch_changed(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Batch(stats)
        if stats.changed_count() > 0 && stats.failed_count() == 0 && stats.skipped_count() == 0)
}

const fn batch_unchanged(result: &TaskResult) -> bool {
    matches!(
        result,
        TaskResult::Batch(stats)
            if stats.changed_count() == 0 && stats.failed_count() == 0 && stats.skipped_count() == 0
                && stats.already_ok_count() > 0
    )
}

// ===========================================================================
// Symlink task execution
// ===========================================================================

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
    let result = InstallSymlinks::new(ec.store.symlinks.clone())
        .run(&ec.ctx)
        .unwrap();
    assert!(batch_changed(&result));

    assert!(
        ec.ctx
            .home()
            .join(".bashrc")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
        "base-section symlink should be installed"
    );
    assert!(
        ec.ctx
            .home()
            .join(".config/Code/User/settings.json")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
        "desktop-section symlink should be installed"
    );
}

// ===========================================================================
// Chmod task execution
// ===========================================================================

#[cfg(unix)]
#[test]
fn chmod_apply_repeat_and_dry_run_preserve_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    for (case, dry_run, expected_mode) in [("apply", false, 0o600), ("dry-run", true, 0o644)] {
        let test = common::TestContextBuilder::new()
            .with_config_file(
                "chmod.toml",
                "[base]\npermissions = [{ path = \"ssh/config\", mode = \"600\" }]\n",
            )
            .build();
        let ec = test.make_context("base");
        let ctx = ec.ctx.with_dry_run(dry_run);
        let target = ctx.home().join(".ssh/config");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "Host *\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

        let task = ApplyFilePermissions::new(ec.store.chmod.clone());
        let result = task.run(&ctx).unwrap();
        assert!(batch_changed(&result), "{case}: {result:?}");
        let mode = || std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(), expected_mode, "{case}");

        if !dry_run {
            let second = task.run(&ctx).unwrap();
            assert!(batch_unchanged(&second), "repeat: {second:?}");
            assert_eq!(mode(), 0o600, "repeat must preserve applied permissions");
        }
    }
}

// ===========================================================================
// Isolated Git config convergence
// ===========================================================================

#[test]
fn git_config_converges_without_mutating_dry_runs_or_current_values() {
    for (case, initial, dry_run, expected, changed) in [
        ("dry-run missing value", None, true, None, true),
        ("incorrect value", Some("true"), false, Some("false"), true),
        ("current value", Some("false"), false, Some("false"), false),
    ] {
        let test = common::TestContextBuilder::new()
            .with_config_file(
                "git-config.toml",
                "[base]\nsettings = [{ key = \"core.autocrlf\", value = \"false\" }]\n",
            )
            .build();
        let ec = test.make_context("base");
        let ctx = ec.ctx.with_dry_run(dry_run);
        let config_path = ctx.home().join("gitconfig");
        std::fs::write(&config_path, "").unwrap();
        if let Some(value) = initial {
            git2::Config::open(&config_path)
                .unwrap()
                .set_str("core.autocrlf", value)
                .unwrap();
        }
        let before = std::fs::read(&config_path).unwrap();

        let settings = ec.store.git_settings.read();
        let resources = settings.iter().map(|setting| {
            GitConfigResource::with_config_path(
                setting.key.clone(),
                setting.value.clone(),
                config_path.clone(),
            )
        });
        let result = process_resources(
            &ctx,
            resources,
            &ProcessOpts::strict("configure").sequential(),
        )
        .unwrap();
        assert!(
            if changed {
                batch_changed(&result)
            } else {
                batch_unchanged(&result)
            },
            "{case}: {result:?}"
        );
        let config = git2::Config::open(&config_path).unwrap();
        match expected {
            Some(value) => assert_eq!(config.get_string("core.autocrlf").unwrap(), value, "{case}"),
            None => assert_eq!(
                config.get_string("core.autocrlf").unwrap_err().code(),
                git2::ErrorCode::NotFound,
                "{case}: absence must not hide a config read error"
            ),
        }
        if dry_run || !changed {
            assert_eq!(
                std::fs::read(&config_path).unwrap(),
                before,
                "{case}: preview/current config must not be rewritten"
            );
        }
    }
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
const FILESYSTEM_TASKS: &[&str] = &["symlinks", "git-hooks", "git"];

#[test]
fn dry_run_pipeline_produces_no_failures() {
    let test = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
        .with_git_hooks_dir()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{ key = \"core.autocrlf\", value = \"false\" }]\n",
        )
        .build();

    let ec = test.make_dry_run_context("base");

    let git_config = ec.ctx.home().join(".gitconfig");
    let original = "[core]\n\tautocrlf = true\n";
    std::fs::write(&git_config, original).unwrap();
    let mut executed = Vec::new();
    for mut task in tasks::all_install_tasks(&ec.store) {
        if FILESYSTEM_TASKS.contains(&task.selector()) {
            if task.selector() == "git" {
                // Native libgit2 is not scoped by Context's injected HOME.
                task = Box::new(tasks::git::git_config::ConfigureGit::with_config_path(
                    ec.store.git_settings.clone(),
                    git_config.clone(),
                ));
            }
            assert!(
                task.should_run(&ec.ctx),
                "{} must be applicable",
                task.selector()
            );
            assert_eq!(
                tasks::execute(task.as_ref(), &ec.ctx),
                test_api::logging::TaskStatus::DryRun,
                "{} must plan a real change",
                task.selector()
            );
            executed.push(task.selector().to_string());
        }
    }
    executed.sort_unstable();
    assert_eq!(
        executed,
        ["git", "git-hooks", "symlinks"],
        "all intended pipeline tasks must execute"
    );
    assert_eq!(ec.log.failure_count(), 0);
    assert!(
        ec.ctx.home().join(".bashrc").symlink_metadata().is_err(),
        "preview must not create a home link"
    );
    assert!(
        !test.root_path().join(".git/hooks/pre-commit").exists(),
        "preview must not install a hook"
    );
    assert_eq!(
        std::fs::read_to_string(git_config).unwrap(),
        original,
        "preview must not rewrite Git config"
    );
}

/// Offline enablement must survive materialization and removal of the checkout.
#[cfg(unix)]
#[test]
fn offline_service_survives_symlink_uninstall_and_checkout_removal() {
    use test_api::exec::{CommandSpec, ExecError, ExecResult, Executor};
    use test_api::tasks::system::systemd_units::ConfigureSystemd;

    #[derive(Debug)]
    struct OfflineSystemdExecutor;

    impl Executor for OfflineSystemdExecutor {
        fn execute(&self, spec: CommandSpec) -> Result<ExecResult, ExecError> {
            common::StubExecutor.execute(spec)
        }

        fn which(&self, program: &str) -> bool {
            program == "systemctl"
        }

        fn which_path(&self, program: &str) -> anyhow::Result<std::path::PathBuf> {
            common::StubExecutor.which_path(program)
        }
    }

    let source = "config/systemd/user/example.service";
    let test = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[linux]\nsymlinks = [\"config/systemd/user/example.service\"]\n",
        )
        .with_config_file(
            "systemd-units.toml",
            "[linux]\nunits = [\"example.service\"]\n",
        )
        .with_symlink_source_content(
            source,
            "[Service]\nExecStart=/bin/true\n[Install]\nWantedBy=default.target\n",
        )
        .build();
    let ec = test.make_context_with_executor(
        "base",
        test_api::platform::Platform {
            os: test_api::platform::Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
        test_api::engine::ContextOpts {
            dry_run: false,
            parallel: false,
            is_ci: Some(false),
        },
        std::sync::Arc::new(OfflineSystemdExecutor),
    );
    let ctx = ec.ctx.with_env(
        test_api::env::MapEnv::new()
            .with("DOTFILES_PROVISIONING", "arch-chroot")
            .into_handle(),
    );
    assert!(batch_changed(
        &InstallSymlinks::new(ec.store.symlinks.clone())
            .run(&ctx)
            .unwrap()
    ));
    let task = ConfigureSystemd::new(ec.store.units.clone());
    let link = ctx
        .home()
        .join(".config/systemd/user/default.target.wants/example.service");
    assert!(batch_changed(&task.run(&ctx.with_dry_run(true)).unwrap()));
    assert!(!link.is_symlink(), "dry-run must not enable the unit");
    assert!(batch_changed(&task.run(&ctx).unwrap()));
    assert!(batch_unchanged(&task.run(&ctx).unwrap()));
    assert!(batch_changed(
        &UninstallSymlinks::new(ec.store.symlinks).run(&ctx).unwrap()
    ));
    let materialized = ctx.home().join(".config/systemd/user/example.service");
    assert!(materialized.is_file() && !materialized.is_symlink());
    std::fs::remove_dir_all(test.root_path().join("symlinks")).unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), materialized);
    assert_eq!(
        std::fs::read_to_string(&link).unwrap(),
        std::fs::read_to_string(&materialized).unwrap()
    );
    assert!(batch_unchanged(&task.run(&ctx).unwrap()));
}
