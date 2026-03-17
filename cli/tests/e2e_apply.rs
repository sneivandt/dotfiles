#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing
)]
//! End-to-end integration tests for the non-dry-run install/apply pipeline.
//!
//! These tests exercise the full `config-load → task-execution →
//! filesystem-outcome` pipeline in a hermetic sandbox:
//!
//! - A minimal dotfiles repository is built under a temporary directory.
//! - A second temporary directory acts as the isolated `$HOME`.
//! - Multiple filesystem-safe install tasks are run together (not in dry-run
//!   mode) using the same [`tasks::execute`] wrapper that the real install
//!   command uses.
//! - Concrete side effects on the sandbox home are asserted after the first
//!   run (real mutations) and after a second run (idempotency).
//!
//! No network access, no package managers, and no real home directory are
//! touched by these tests.

mod common;

#[cfg(unix)]
mod unix_e2e {
    use super::common;
    use dotfiles_cli::tasks;
    use dotfiles_cli::tasks::apply::chmod::ApplyFilePermissions;
    use dotfiles_cli::tasks::apply::symlinks::InstallSymlinks;
    use dotfiles_cli::tasks::repository::hooks::InstallGitHooks;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Build a test execution context that has:
    ///
    /// - One symlink (`bashrc`) declared in `conf/symlinks.toml`.
    /// - A `pre-commit` hook source in `hooks/`.
    /// - A `.git/hooks/` directory so the hook task can install.
    /// - A chmod permission entry for `.ssh/config` in `conf/chmod.toml`.
    ///
    /// The helper also creates the `ssh/config` file in the sandbox home so
    /// the chmod task has a target to operate on.
    ///
    /// Returns `(test_repo, execution_context)`.  Both must stay alive for the
    /// duration of the test — `test_repo` owns the repository root and
    /// `execution_context` owns the sandbox home.
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

        // Pre-create the ssh/config file that ApplyFilePermissions targets.
        let ssh_config = ec.ctx.home.join(".ssh/config");
        std::fs::create_dir_all(ssh_config.parent().unwrap()).unwrap();
        std::fs::write(&ssh_config, "Host *\n").unwrap();

        (test, ec)
    }

    // -----------------------------------------------------------------------
    // Full non-dry-run pipeline
    // -----------------------------------------------------------------------

    /// Running the complete set of filesystem-safe install tasks in non-dry-run
    /// mode must produce all expected side effects in the sandbox home.
    ///
    /// Specifically this test validates:
    /// 1. `InstallSymlinks` creates a symlink in the sandbox home pointing to
    ///    the source inside the repository's `symlinks/` directory.
    /// 2. `InstallGitHooks` writes the hook file to `.git/hooks/`.
    /// 3. `ApplyFilePermissions` sets the declared mode on the target file.
    /// 4. No task records a failure.
    #[test]
    fn apply_pipeline_produces_expected_filesystem_state() {
        use std::os::unix::fs::PermissionsExt as _;

        let (test, ec) = build_full_fixture();

        tasks::execute(&InstallSymlinks, &ec.ctx);
        tasks::execute(&InstallGitHooks::new(), &ec.ctx);
        tasks::execute(&ApplyFilePermissions, &ec.ctx);

        assert!(!ec.log.has_failures(), "no task should fail");

        // 1. Symlink created and points to the repository source.
        let link = ec.ctx.home.join(".bashrc");
        assert!(
            link.symlink_metadata().is_ok(),
            "symlink .bashrc should exist in sandbox home"
        );
        assert!(
            link.symlink_metadata().unwrap().is_symlink(),
            ".bashrc should be a symlink, not a regular file"
        );
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(
            target,
            test.root_path().join("symlinks/bashrc"),
            "symlink should point to the source in the repository"
        );

        // 2. Hook installed in the repository's .git/hooks/ directory.
        let hook = test.root_path().join(".git/hooks/pre-commit");
        assert!(
            hook.exists(),
            "pre-commit hook should be installed in .git/hooks/"
        );

        // 3. File permissions applied to the sandbox home target.
        let mode = std::fs::metadata(ec.ctx.home.join(".ssh/config"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "ssh/config should have mode 0600 after apply");
    }

    // -----------------------------------------------------------------------
    // Idempotency
    // -----------------------------------------------------------------------

    /// Running the same pipeline twice must succeed without errors and leave
    /// the filesystem in an identical state.
    ///
    /// This is the key idempotency guarantee: a second `dotfiles install` must
    /// not break anything that the first run already set up correctly.
    #[test]
    fn apply_pipeline_is_idempotent() {
        use std::os::unix::fs::PermissionsExt as _;

        let (test, ec) = build_full_fixture();

        // ── First run ───────────────────────────────────────────────────────
        tasks::execute(&InstallSymlinks, &ec.ctx);
        tasks::execute(&InstallGitHooks::new(), &ec.ctx);
        tasks::execute(&ApplyFilePermissions, &ec.ctx);
        assert!(
            !ec.log.has_failures(),
            "first run should produce no failures"
        );

        // ── Second run (same context, same tasks) ────────────────────────────
        tasks::execute(&InstallSymlinks, &ec.ctx);
        tasks::execute(&InstallGitHooks::new(), &ec.ctx);
        tasks::execute(&ApplyFilePermissions, &ec.ctx);
        assert!(
            !ec.log.has_failures(),
            "second (idempotent) run should also produce no failures"
        );

        // Filesystem state must still be correct after the second run.
        let link = ec.ctx.home.join(".bashrc");
        assert!(
            link.symlink_metadata().unwrap().is_symlink(),
            ".bashrc should still be a symlink after idempotent run"
        );
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            test.root_path().join("symlinks/bashrc"),
            "symlink target should be unchanged after idempotent run"
        );
        assert!(
            test.root_path().join(".git/hooks/pre-commit").exists(),
            "hook should still be present after idempotent run"
        );
        let mode = std::fs::metadata(ec.ctx.home.join(".ssh/config"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "permissions should still be 0600 after idempotent run"
        );
    }

    // -----------------------------------------------------------------------
    // Symlink content reachable through link
    // -----------------------------------------------------------------------

    /// The symlink created by the pipeline must resolve to the correct content
    /// (i.e., reading through the symlink returns the source file's content).
    #[test]
    fn apply_pipeline_symlink_resolves_to_source_content() {
        let (_test, ec) = build_full_fixture();

        tasks::execute(&InstallSymlinks, &ec.ctx);
        assert!(!ec.log.has_failures());

        let content = std::fs::read_to_string(ec.ctx.home.join(".bashrc")).unwrap();
        assert_eq!(
            content, "# bash config\n",
            "reading through symlink should return source file content"
        );
    }
}
