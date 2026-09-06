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

#[cfg(unix)]
mod unix_e2e {
    use super::common;
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

    fn install(ec: &common::ExecutionContext, ctx: &tasks::Context) {
        tasks::execute(&InstallSymlinks::new(ec.store.symlinks.clone()), ctx);
        tasks::execute(&InstallGitHooks::new(), ctx);
        tasks::execute(&ApplyFilePermissions::new(ec.store.chmod.clone()), ctx);
        assert_eq!(ec.log.failure_count(), 0, "install must not fail");
    }

    fn uninstall(ec: &common::ExecutionContext, ctx: &tasks::Context) {
        tasks::execute(&UninstallSymlinks::new(ec.store.symlinks.clone()), ctx);
        tasks::execute(&UninstallGitHooks::new(), ctx);
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

        install(&ec, &dry_run);
        assert!(
            link.symlink_metadata().is_err(),
            "preview must not create a link"
        );
        assert!(!hook.exists(), "preview must not install a hook");
        assert_eq!(permissions(&ssh_config), 0o644, "preview must not chmod");
        assert_eq!(std::fs::read_to_string(&ssh_config).unwrap(), "Host *\n");

        for pass in ["initial install", "repeat install"] {
            install(&ec, &ec.ctx);
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
        uninstall(&ec, &dry_run);
        assert!(
            link.symlink_metadata().unwrap().is_symlink(),
            "uninstall preview must not materialize the link"
        );
        assert_eq!(std::fs::read(&hook).unwrap(), installed_hook);

        for pass in ["initial uninstall", "repeat uninstall"] {
            uninstall(&ec, &ec.ctx);
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
