#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! End-to-end convergence against a temporary repository and home.
//!
//! Exercise configuration loading, real task execution, dry-run safety,
//! repeated installation, and conservative removal without system commands.

mod common;

#[test]
fn agent_settings_overlay_preview_and_repeat_preserve_unmanaged_values() {
    for with_overlay in [false, true] {
        let test = common::TestContextBuilder::new()
            .with_config_file(
                "agent-settings.toml",
                "[base]\nsettings = [\
                 { target = 'copilot', key = 'model', value = 'baseline' },\
                 { target = 'copilot', key = 'beep', value = false }]\n",
            )
            .build();
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir(overlay.path().join("conf")).unwrap();
        std::fs::write(
            overlay.path().join("conf/agent-settings.toml"),
            "[base]\nsettings = [{ target = 'copilot', key = 'model', value = 'work-model' }]\n",
        )
        .unwrap();
        let settings_path = home.path().join(".copilot/settings.json");
        std::fs::create_dir(settings_path.parent().unwrap()).unwrap();
        let original = "{\"model\":\"original\",\"unmanaged\":true}\n";
        std::fs::write(&settings_path, original).unwrap();
        let overlay_path = with_overlay.then_some(overlay.path());
        for dry_run in [true, false, false] {
            let mut command = common::cli_command(
                test.root_path(),
                home.path(),
                overlay_path,
                "update",
                "agent-settings",
            );
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "overlay={with_overlay}, dry_run={dry_run}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = std::fs::read_to_string(&settings_path).unwrap();
            if dry_run {
                assert_eq!(actual, original, "preview must not change settings");
            } else {
                let expected_model = if with_overlay {
                    "work-model"
                } else {
                    "baseline"
                };
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
                    serde_json::json!({
                        "model": expected_model,
                        "beep": false,
                        "unmanaged": true,
                    })
                );
            }
        }
    }
}

#[cfg(unix)]
mod unix_e2e {
    use super::common;
    use dotfiles_cli::testing::logging::TaskStatus;
    use dotfiles_cli::testing::tasks;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use tasks::files::chmod::ApplyFilePermissions;
    use tasks::files::symlinks::{InstallSymlinks, UninstallSymlinks};
    use tasks::git::hooks::{InstallGitHooks, UninstallGitHooks};

    fn build_full_fixture() -> (common::IntegrationTestContext, common::ExecutionContext) {
        let test = common::TestContextBuilder::new()
            .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
            .with_symlink_source_content("bashrc", "# bash config\n")
            .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
            .with_git_hooks_dir()
            .with_config_file(
                "chmod.toml",
                "[base]\npermissions = [{ path = \"ssh/config\", mode = \"600\" }]\n",
            )
            .build();
        let ec = test.make_context("base");
        let ssh_config = ec.ctx.home().join(".ssh/config");
        std::fs::create_dir_all(ssh_config.parent().unwrap()).unwrap();
        std::fs::write(&ssh_config, "Host *\n").unwrap();
        std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644)).unwrap();
        (test, ec)
    }

    fn install(ec: &common::ExecutionContext, ctx: &tasks::Context, expected: TaskStatus) {
        assert_eq!(
            tasks::execute(&InstallSymlinks::new(ec.store.symlinks.clone()), ctx),
            expected,
            "symlinks"
        );
        assert_eq!(
            tasks::execute(&InstallGitHooks::new(), ctx),
            expected,
            "hooks"
        );
        assert_eq!(
            tasks::execute(&ApplyFilePermissions::new(ec.store.chmod.clone()), ctx),
            expected,
            "permissions"
        );
        assert_eq!(ec.log.failure_count(), 0, "install must not fail");
    }

    fn uninstall(ec: &common::ExecutionContext, ctx: &tasks::Context, expected: TaskStatus) {
        assert_eq!(
            tasks::execute(&UninstallSymlinks::new(ec.store.symlinks.clone()), ctx),
            expected,
            "symlinks"
        );
        assert_eq!(
            tasks::execute(&UninstallGitHooks::new(), ctx),
            expected,
            "hooks"
        );
        assert_eq!(ec.log.failure_count(), 0, "uninstall must not fail");
    }

    fn permissions(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn install_preview_repeat_and_uninstall_preserve_the_lifecycle() {
        let (test, ec) = build_full_fixture();
        let dry_run = ec.ctx.with_dry_run(true);
        let link = ec.ctx.home().join(".bashrc");
        let hook = test.root_path().join(".git/hooks/pre-commit");
        let ssh_config = ec.ctx.home().join(".ssh/config");

        install(&ec, &dry_run, TaskStatus::DryRun);
        assert!(
            link.symlink_metadata().is_err(),
            "preview must not create a link"
        );
        assert!(!hook.exists(), "preview must not install a hook");
        assert_eq!(permissions(&ssh_config), 0o644, "preview must not chmod");
        assert_eq!(std::fs::read_to_string(&ssh_config).unwrap(), "Host *\n");

        for (pass, expected) in [
            ("initial install", TaskStatus::Changed),
            ("repeat install", TaskStatus::Ok),
        ] {
            install(&ec, &ec.ctx, expected);
            assert!(
                link.symlink_metadata().unwrap().is_symlink(),
                "{pass}: .bashrc must be a symlink"
            );
            assert_eq!(
                std::fs::read_link(&link).unwrap(),
                test.root_path().join("symlinks/bashrc"),
                "{pass}: link target"
            );
            assert_eq!(
                std::fs::read_to_string(&link).unwrap(),
                "# bash config\n",
                "{pass}: source content must be readable through the link"
            );
            assert!(hook.is_file(), "{pass}: hook must exist");
            assert_eq!(permissions(&ssh_config), 0o600, "{pass}: configured mode");
        }

        let installed_hook = std::fs::read(&hook).unwrap();
        uninstall(&ec, &dry_run, TaskStatus::DryRun);
        assert!(
            link.symlink_metadata().unwrap().is_symlink(),
            "uninstall preview must not materialize the link"
        );
        assert_eq!(std::fs::read(&hook).unwrap(), installed_hook);

        for (pass, expected) in [
            ("initial uninstall", TaskStatus::Changed),
            ("repeat uninstall", TaskStatus::Ok),
        ] {
            uninstall(&ec, &ec.ctx, expected);
            assert!(
                !link.symlink_metadata().unwrap().is_symlink(),
                "{pass}: managed link must be materialized"
            );
            assert_eq!(
                std::fs::read_to_string(&link).unwrap(),
                "# bash config\n",
                "{pass}: materialized content must survive"
            );
            assert!(!hook.exists(), "{pass}: managed hook must be removed");
            assert_eq!(
                permissions(&ssh_config),
                0o600,
                "{pass}: retain permissions"
            );
            assert_eq!(
                std::fs::read_to_string(&ssh_config).unwrap(),
                "Host *\n",
                "{pass}: retain unrelated user files"
            );
        }
    }
}
