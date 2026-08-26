//! Unit tests for sparse checkout configuration.

use super::*;
use crate::infra::ConfigHandle;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor};
use crate::infra::fs::MockFileSystemOps;
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ScriptedExecutor, empty_config, make_context, make_linux_context};
use std::path::PathBuf;
use std::sync::Arc;

// -----------------------------------------------------------------------
// build_patterns
// -----------------------------------------------------------------------

#[test]
fn build_patterns_no_exclusions() {
    let patterns = build_patterns(&[]);
    assert_eq!(patterns, "/*");
}

#[test]
fn build_patterns_single_exclusion() {
    let patterns = build_patterns(&["AppData/".to_string()]);
    assert_eq!(patterns, "/*\n!/symlinks/AppData/");
}

#[test]
fn build_patterns_multiple_exclusions() {
    let patterns = build_patterns(&["AppData/".to_string(), "config/git/windows".to_string()]);
    assert_eq!(
        patterns,
        "/*\n!/symlinks/AppData/\n!/symlinks/config/git/windows"
    );
}

// -----------------------------------------------------------------------
// is_up_to_date
// -----------------------------------------------------------------------

#[test]
fn is_up_to_date_false_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse-checkout");
    assert!(!is_up_to_date(&path, "/*\n!/symlinks"));
}

#[test]
fn is_up_to_date_true_when_content_matches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse-checkout");
    let patterns = "/*\n!/symlinks";
    std::fs::write(&path, patterns).unwrap();
    assert!(is_up_to_date(&path, patterns));
}

#[test]
fn is_up_to_date_true_ignores_trailing_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse-checkout");
    std::fs::write(&path, "/*\n!/symlinks\n").unwrap();
    assert!(is_up_to_date(&path, "/*\n!/symlinks"));
}

#[test]
fn is_up_to_date_false_when_content_differs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse-checkout");
    std::fs::write(&path, "/*").unwrap();
    assert!(!is_up_to_date(&path, "/*\n!/symlinks"));
}

#[test]
fn restore_sparse_checkout_file_removes_file_when_previous_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse-checkout");
    std::fs::write(&path, "/*\n!/symlinks").unwrap();

    restore_sparse_checkout_file(&path, None).unwrap();

    assert!(!path.exists());
}

#[test]
fn reset_excluded_to_head_uses_symlinks_repo_paths() {
    let dir = tempfile::tempdir().unwrap();
    let tracked_path = dir.path().join("symlinks").join("config").join("nvim");
    std::fs::create_dir_all(tracked_path.parent().unwrap()).unwrap();
    std::fs::write(&tracked_path, "tracked").unwrap();

    let executor = ScriptedExecutor::new().git(
        dir.path(),
        &["checkout", "HEAD", "--", "symlinks/config/nvim"],
        "",
    );
    let config = empty_config(dir.path().to_path_buf());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    reset_excluded_to_head(
        &ctx,
        dir.path(),
        &["config/nvim".to_string(), "missing/file".to_string()],
    );
}

// -----------------------------------------------------------------------
// should_run
// -----------------------------------------------------------------------

#[test]
fn should_run_false_when_git_dir_missing() {
    let config = empty_config(PathBuf::from("/nonexistent/repo"));
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_linux_context(config);
    assert!(!ConfigureSparseCheckout::new(manifest).should_run(&ctx));
}

#[test]
fn should_run_true_when_git_dir_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let config = empty_config(dir.path().to_path_buf());
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_linux_context(config);
    assert!(ConfigureSparseCheckout::new(manifest).should_run(&ctx));
}

// -----------------------------------------------------------------------
// is_broken_symlink_into
// -----------------------------------------------------------------------

#[test]
fn is_broken_symlink_into_false_for_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("regular");
    std::fs::write(&file, "content").unwrap();
    assert!(!is_broken_symlink_into(
        &SystemFileSystemOps,
        &file,
        dir.path()
    ));
}

#[cfg(unix)]
#[test]
fn is_broken_symlink_into_false_for_valid_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    std::fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    // Valid symlink (target exists) → not broken
    assert!(!is_broken_symlink_into(
        &SystemFileSystemOps,
        &link,
        dir.path()
    ));
}

#[cfg(unix)]
#[test]
fn is_broken_symlink_into_true_for_dangling_symlink_into_dir() {
    let dir = tempfile::tempdir().unwrap();
    let symlinks_dir = dir.path().join("symlinks");
    std::fs::create_dir(&symlinks_dir).unwrap();
    let link = dir.path().join("link");
    // Point into symlinks_dir at a path that does not exist
    let nonexistent = symlinks_dir.join("missing");
    std::os::unix::fs::symlink(&nonexistent, &link).unwrap();
    assert!(is_broken_symlink_into(
        &SystemFileSystemOps,
        &link,
        &symlinks_dir
    ));
}

// -----------------------------------------------------------------------
// ConfigureSparseCheckout::run
// -----------------------------------------------------------------------

#[test]
fn run_returns_ok_when_no_excluded_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = empty_config(dir.path().to_path_buf());
    let manifest = ConfigHandle::new(config.manifest.clone());
    let executor = ScriptedExecutor::new().git_unchecked_result(
        dir.path(),
        &["config", "--get", "core.sparseCheckout"],
        Ok(ExecResult::failure("", "", Some(1))),
    );
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn run_disables_sparse_checkout_when_exclusions_become_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let config = empty_config(dir.path().to_path_buf());
    let manifest = ConfigHandle::new(config.manifest.clone());
    let executor = ScriptedExecutor::new()
        .git_unchecked(
            dir.path(),
            &["config", "--get", "core.sparseCheckout"],
            "true\n",
        )
        .git(
            dir.path(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git(dir.path(), &["sparse-checkout", "disable"], "");
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();

    assert!(matches!(result, TaskResult::Batch(stats) if stats.changed_count() == 1));
}

#[test]
fn run_returns_ok_when_already_up_to_date() {
    let dir = tempfile::tempdir().unwrap();

    // Write the exact patterns that build_patterns would produce
    let info_dir = dir.path().join(".git").join("info");
    std::fs::create_dir_all(&info_dir).unwrap();
    std::fs::write(info_dir.join("sparse-checkout"), "/*\n!/symlinks/AppData/").unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("AppData/".to_string());

    // core.sparseCheckout reports enabled, so the file match short-circuits.
    let executor = ScriptedExecutor::new().git_unchecked(
        dir.path(),
        &["config", "--get", "core.sparseCheckout"],
        "true\n",
    );
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok when sparse-checkout is already up to date, got {result:?}"
    );
}

#[test]
fn run_reconfigures_when_file_matches_but_config_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let info_dir = dir.path().join(".git").join("info");
    std::fs::create_dir_all(&info_dir).unwrap();
    // File already matches the expected patterns for "symlinks".
    std::fs::write(info_dir.join("sparse-checkout"), "/*\n!/symlinks/symlinks").unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());

    let executor = ScriptedExecutor::new()
        // core.sparseCheckout is disabled (key unset → non-zero exit);
        .git_unchecked_result(
            dir.path(),
            &["config", "--get", "core.sparseCheckout"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        // worktree_has_local_changes: clean.
        .git(
            dir.path(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        // extensions.worktreeConfig is likewise unset, so config writes go
        // to the repository scope.
        .git_unchecked_result(
            dir.path(),
            &["config", "--get", "extensions.worktreeConfig"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckout"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckoutCone"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        // config core.sparseCheckout true, config core.sparseCheckoutCone
        // false, then read-tree (the excluded "symlinks" path is absent, so
        // no checkout).
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckoutCone", "false"],
            "",
        )
        .git(dir.path(), &["read-tree", "-mu", "HEAD"], "");
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected reconfigure (not early Ok) when config is disabled, got {result:?}"
    );
}

#[test]
fn run_returns_planned_change_when_patterns_need_update() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    // No sparse-checkout file means the patterns would change.

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());
    let manifest = ConfigHandle::new(config.manifest.clone());
    let executor = ScriptedExecutor::new().git(
        dir.path(),
        &["status", "--porcelain", "--untracked-files=no"],
        "",
    );
    let mut ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    ctx = ctx.with_dry_run(true);

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "expected planned change, got {result:?}"
    );
}

#[test]
fn run_skips_when_worktree_has_local_changes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());

    let executor = ScriptedExecutor::new().git(
        dir.path(),
        &["status", "--porcelain", "--untracked-files=no"],
        "M  cli/src/tasks/packages.rs\n",
    );
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::Skipped { ref reason, .. }
                if reason.contains("local changes present")
        ),
        "expected local changes skip, got {result:?}"
    );
}

#[test]
fn post_update_reconciliation_fails_when_local_changes_block_sparse_checkout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());
    let executor = ScriptedExecutor::new().git(
        dir.path(),
        &["status", "--porcelain", "--untracked-files=no"],
        "M  cli/src/tasks/packages.rs\n",
    );
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest)
        .fail_if_skipped(true)
        .run(&ctx)
        .unwrap();

    assert!(
        matches!(
            result,
            TaskResult::Failed(ref reason)
                if reason.contains("post-update sparse checkout reconciliation skipped")
        ),
        "post-update reconciliation must fail rather than continue from an inconsistent checkout"
    );
}

#[test]
fn dry_run_skips_when_worktree_has_local_changes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());

    let executor = ScriptedExecutor::new().git(
        dir.path(),
        &["status", "--porcelain", "--untracked-files=no"],
        "M  cli/src/tasks/packages.rs\n",
    );
    let manifest = ConfigHandle::new(config.manifest.clone());
    let mut ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    ctx = ctx.with_dry_run(true);

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::Skipped { ref reason, .. }
                if reason.contains("local changes present")
        ),
        "expected local changes skip, got {result:?}"
    );
}

#[test]
fn run_removes_broken_git_symlinks_before_applying_sparse_checkout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config
        .manifest
        .excluded_files
        .push("config/git/config".to_string());

    let mut seq = mockall::Sequence::new();
    let broken_link = PathBuf::from("/home/test/.config/git/config");
    let target = dir
        .path()
        .join("symlinks")
        .join("config")
        .join("git")
        .join("config");

    let mut fs = MockFileSystemOps::new();
    fs.expect_exists()
        .once()
        .in_sequence(&mut seq)
        .withf(|p| p == Path::new("/home/test/.config/git"))
        .returning(|_| true);
    let broken_link_for_read_dir = broken_link.clone();
    fs.expect_read_dir()
        .once()
        .in_sequence(&mut seq)
        .withf(|p| p == Path::new("/home/test/.config/git"))
        .returning(move |_| Ok(vec![broken_link_for_read_dir.clone()]));
    let broken_link_for_read_link = broken_link.clone();
    let target_for_read_link = target.clone();
    fs.expect_read_link()
        .once()
        .in_sequence(&mut seq)
        .withf(move |p| p == broken_link_for_read_link.as_path())
        .returning(move |_| Ok(target_for_read_link.clone()));
    let target_for_exists = target;
    fs.expect_exists()
        .once()
        .in_sequence(&mut seq)
        .withf(move |p| p == target_for_exists.as_path())
        .returning(|_| false);
    fs.expect_remove()
        .once()
        .in_sequence(&mut seq)
        .withf(move |p| p == broken_link.as_path())
        .returning(|_| Ok(()));

    let executor = ScriptedExecutor::new()
        .git(
            dir.path(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--get", "extensions.worktreeConfig"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckout"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckoutCone"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckoutCone", "false"],
            "",
        )
        .git(dir.path(), &["read-tree", "-mu", "HEAD"], "");

    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(fs), manifest);

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected sparse checkout apply, got {result:?}"
    );
}

#[derive(Debug)]
struct UntrackedAwareExecutor;

impl Executor for UntrackedAwareExecutor {
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        let stdout = if spec
            .arguments()
            .iter()
            .any(|arg| arg == "--untracked-files=no")
        {
            String::new()
        } else {
            "?? scratch.txt\n".to_string()
        };

        Ok(ExecResult::success(stdout))
    }

    fn which(&self, _: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> Result<PathBuf> {
        anyhow::bail!("{program} not found on PATH")
    }
}

#[test]
fn worktree_has_local_changes_ignores_untracked_files() {
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_context(
        config,
        Platform::new(Os::Linux, false),
        Arc::new(UntrackedAwareExecutor),
    );

    assert!(!worktree_has_local_changes(&ctx).unwrap());
}

#[test]
fn enable_sparse_checkout_uses_worktree_scope_when_extension_enabled() {
    let executor = ScriptedExecutor::new()
        .git_unchecked(
            "/repo",
            &["config", "--get", "extensions.worktreeConfig"],
            "true\n",
        )
        .git(
            "/repo",
            &["config", "--worktree", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            "/repo",
            &["config", "--worktree", "core.sparseCheckoutCone", "false"],
            "",
        );
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    enable_sparse_checkout_config(&ctx, Path::new("/repo")).unwrap();
}

#[test]
fn enable_sparse_checkout_uses_repo_scope_when_extension_disabled() {
    let executor = ScriptedExecutor::new()
        // extensions.worktreeConfig is unset (non-zero exit), matching real
        // git behaviour for a missing key.
        .git_unchecked_result(
            "/repo",
            &["config", "--get", "extensions.worktreeConfig"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git(
            "/repo",
            &["config", "--local", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            "/repo",
            &["config", "--local", "core.sparseCheckoutCone", "false"],
            "",
        );
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));
    enable_sparse_checkout_config(&ctx, Path::new("/repo")).unwrap();
}

#[test]
fn run_writes_sparse_checkout_patterns_and_calls_git() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());

    // The excluded file "symlinks" does NOT exist on disk, so the
    // `git checkout HEAD -- <files>` step is skipped. Calls, in order:
    //   1. git status --porcelain --untracked-files=no
    //   2. git config --get extensions.worktreeConfig (unset)
    //   3. git config core.sparseCheckout true
    //   4. git config core.sparseCheckoutCone false
    //   5. git read-tree -mu HEAD
    let executor = ScriptedExecutor::new()
        .git(
            dir.path(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--get", "extensions.worktreeConfig"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckout"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckoutCone"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckoutCone", "false"],
            "",
        )
        .git(dir.path(), &["read-tree", "-mu", "HEAD"], "");
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let result = ConfigureSparseCheckout::new(manifest).run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Ok),
        "expected Ok after writing sparse-checkout, got {result:?}"
    );

    // Verify the sparse-checkout file was written with the expected patterns
    let sparse_file = dir.path().join(".git").join("info").join("sparse-checkout");
    let content = std::fs::read_to_string(&sparse_file).unwrap();
    assert!(
        content.contains("/*"),
        "must include default inclusion pattern"
    );
    assert!(
        content.contains("!/symlinks"),
        "must include exclusion pattern for 'symlinks'"
    );
}

#[test]
fn run_restores_previous_sparse_checkout_file_when_read_tree_fails() {
    let dir = tempfile::tempdir().unwrap();
    let info_dir = dir.path().join(".git").join("info");
    std::fs::create_dir_all(&info_dir).unwrap();
    let sparse_file = info_dir.join("sparse-checkout");
    let previous_patterns = "/*\n!/old-path";
    std::fs::write(&sparse_file, previous_patterns).unwrap();

    let mut config = empty_config(dir.path().to_path_buf());
    config.manifest.excluded_files.push("symlinks".to_string());

    // Calls in order:
    //   1. git status → clean
    //   2. git config --get extensions.worktreeConfig → unset (repo scope)
    //   3. git config core.sparseCheckout → success
    //   4. git config core.sparseCheckoutCone → success
    //   5. git read-tree → fails
    //   6. rollback git read-tree → success
    let executor = ScriptedExecutor::new()
        .git(
            dir.path(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        )
        .git_unchecked_result(
            dir.path(),
            &["config", "--get", "extensions.worktreeConfig"],
            Ok(ExecResult::failure("", "", Some(1))),
        )
        .git_unchecked(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckout"],
            "false\n",
        )
        .git_unchecked(
            dir.path(),
            &["config", "--local", "--get", "core.sparseCheckoutCone"],
            "true\n",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckout", "true"],
            "",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckoutCone", "false"],
            "",
        )
        .git_result(
            dir.path(),
            &["read-tree", "-mu", "HEAD"],
            Err(ExecError::spawn(
                "git",
                std::io::Error::other("mock read-tree failed"),
            )),
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckout", "false"],
            "",
        )
        .git(
            dir.path(),
            &["config", "--local", "core.sparseCheckoutCone", "true"],
            "",
        )
        .git(dir.path(), &["read-tree", "-mu", "HEAD"], "");
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

    let err = ConfigureSparseCheckout::new(manifest)
        .run(&ctx)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("applying sparse-checkout patterns"),
        "expected read-tree failure to be surfaced, got {err:#}"
    );
    let restored = std::fs::read_to_string(&sparse_file).unwrap();
    assert_eq!(restored, previous_patterns);
}

// -----------------------------------------------------------------------
// remove_broken_git_symlinks — using MockFileSystemOps
// -----------------------------------------------------------------------

#[test]
fn remove_broken_git_symlinks_skips_when_git_config_dir_missing() {
    // ~/.config/git does not exist → function should return immediately
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_linux_context(config);
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists().returning(|_| false);
    let fs = Arc::new(mock);
    // Should not panic or remove anything
    remove_broken_git_symlinks(&ctx, &*fs);
}

#[test]
fn remove_broken_git_symlinks_skips_valid_symlinks() {
    // Symlink whose target exists → must not be removed
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_linux_context(config);
    let symlink_path = PathBuf::from("/home/test/.config/git/gitconfig");
    let target = PathBuf::from("/repo/symlinks/git/gitconfig");
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists()
        .withf(|p| p == Path::new("/home/test/.config/git"))
        .returning(|_| true);
    mock.expect_exists()
        .withf(|p| p == Path::new("/repo/symlinks/git/gitconfig"))
        .returning(|_| true); // target exists → not broken
    mock.expect_read_dir()
        .returning(move |_| Ok(vec![symlink_path.clone()]));
    mock.expect_read_link()
        .returning(move |_| Ok(target.clone()));
    // expect_remove must NOT be called — mockall will panic if it is
    let fs = Arc::new(mock);
    remove_broken_git_symlinks(&ctx, &*fs);
}

#[test]
fn remove_broken_git_symlinks_removes_dangling_symlinks_pointing_into_symlinks_dir() {
    // Symlink whose target is under symlinks_dir and does not exist → must be removed
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_linux_context(config);
    let symlink_path = PathBuf::from("/home/test/.config/git/gitconfig");
    let target = PathBuf::from("/repo/symlinks/git/gitconfig");
    let read_dir_entry = symlink_path.clone();
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists()
        .withf(|p| p == Path::new("/home/test/.config/git"))
        .returning(|_| true);
    mock.expect_exists()
        .withf(|p| p == Path::new("/repo/symlinks/git/gitconfig"))
        .returning(|_| false); // target doesn't exist → broken
    mock.expect_read_dir()
        .returning(move |_| Ok(vec![read_dir_entry.clone()]));
    mock.expect_read_link()
        .returning(move |_| Ok(target.clone()));
    mock.expect_remove()
        .withf(move |p| p == symlink_path.as_path())
        .returning(|_| Ok(()))
        .times(1);
    let fs = Arc::new(mock);
    remove_broken_git_symlinks(&ctx, &*fs);
}

#[test]
fn remove_broken_git_symlinks_ignores_regular_files() {
    // A regular file (not a symlink) must never be removed
    let config = empty_config(PathBuf::from("/repo"));
    let ctx = make_linux_context(config);
    let file_path = PathBuf::from("/home/test/.config/git/config");
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists()
        .withf(|p| p == Path::new("/home/test/.config/git"))
        .returning(|_| true);
    mock.expect_read_dir()
        .returning(move |_| Ok(vec![file_path.clone()]));
    mock.expect_read_link()
        .returning(|_| Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)));
    // expect_remove must NOT be called
    let fs = Arc::new(mock);
    remove_broken_git_symlinks(&ctx, &*fs);
}

// -----------------------------------------------------------------------
// should_run — using MockFileSystemOps
// -----------------------------------------------------------------------

#[test]
fn should_run_false_when_git_dir_missing_via_mock() {
    let config = empty_config(PathBuf::from("/repo"));
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_linux_context(config);
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists().returning(|_| false);
    let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(mock), manifest);
    assert!(!task.should_run(&ctx));
}

#[test]
fn should_run_true_when_git_dir_exists_via_mock() {
    let config = empty_config(PathBuf::from("/repo"));
    let manifest = ConfigHandle::new(config.manifest.clone());
    let ctx = make_linux_context(config);
    let mut mock = MockFileSystemOps::new();
    mock.expect_exists().returning(|_| true);
    let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(mock), manifest);
    assert!(task.should_run(&ctx));
}
