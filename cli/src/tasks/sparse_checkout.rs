//! Task: configure sparse checkout.
use anyhow::{Context as _, Result};
use std::path::Path;
use std::sync::Arc;

use super::{Context, Task, TaskResult};
use crate::fs::{FileSystemOps, SystemFileSystemOps};

/// Default sparse checkout pattern that includes all files at root level.
const DEFAULT_SPARSE_PATTERN: &str = "/*";

/// Build the sparse checkout pattern string from excluded files.
fn build_patterns(excluded_files: &[String]) -> String {
    let mut patterns = vec![DEFAULT_SPARSE_PATTERN.to_string()];
    for file in excluded_files {
        patterns.push(format!("!/{file}"));
    }
    patterns.join("\n")
}

/// Check if the sparse-checkout file is already up to date with the given patterns.
fn is_up_to_date(sparse_file: &Path, patterns_str: &str) -> bool {
    if !sparse_file.exists() {
        return false;
    }
    std::fs::read_to_string(sparse_file)
        .map(|current| current.trim() == patterns_str.trim())
        .unwrap_or(false)
}

fn restore_sparse_checkout_file(sparse_file: &Path, previous_patterns: Option<&str>) -> Result<()> {
    if let Some(previous) = previous_patterns {
        std::fs::write(sparse_file, previous).with_context(|| {
            format!(
                "restoring sparse-checkout file at {}",
                sparse_file.display()
            )
        })
    } else {
        if let Err(err) = std::fs::remove_file(sparse_file)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(err).with_context(|| {
                format!("removing sparse-checkout file at {}", sparse_file.display())
            });
        }
        Ok(())
    }
}

/// Remove broken symlinks in `~/.config/git/` that point into the dotfiles
/// repo's `symlinks/` directory.  These become dangling when sparse-checkout
/// excludes `symlinks/`, which then prevents git from running at all because
/// it cannot read its own XDG config / exclude files.
fn remove_broken_git_symlinks(ctx: &Context, fs: &dyn FileSystemOps) {
    let git_config_dir = ctx.home.join(".config").join("git");
    if !fs.exists(&git_config_dir) {
        return;
    }
    let symlinks_dir = ctx.symlinks_dir();
    let Ok(entries) = fs.read_dir(&git_config_dir) else {
        return;
    };
    for path in entries {
        if !is_broken_symlink_into(fs, &path, &symlinks_dir) {
            continue;
        }
        ctx.log.debug(&format!(
            "removing broken git config symlink: {}",
            path.display()
        ));
        if let Err(e) = fs.remove(&path) {
            ctx.log.debug(&format!("failed to remove symlink: {e}"));
        }
    }
}

/// Returns true when `path` is a symlink whose target lives under `dir` and
/// the target does not exist on disk.
fn is_broken_symlink_into(fs: &dyn FileSystemOps, path: &Path, dir: &Path) -> bool {
    fs.read_link(path).is_ok_and(|target| {
        // Resolve relative symlink targets relative to the symlink's directory
        let resolved_target = if target.is_absolute() {
            target
        } else {
            path.parent()
                .map_or_else(|| target.clone(), |parent| parent.join(&target))
        };
        resolved_target.starts_with(dir) && !fs.exists(&resolved_target)
    })
}

/// Configure git sparse checkout based on the profile manifest.
#[derive(Debug)]
pub struct ConfigureSparseCheckout {
    fs_ops: Arc<dyn FileSystemOps>,
}

impl ConfigureSparseCheckout {
    /// Create using the real filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fs_ops: Arc::new(SystemFileSystemOps),
        }
    }

    /// Create with a custom [`FileSystemOps`] implementation (for testing).
    #[cfg(test)]
    pub fn with_fs_ops(fs_ops: Arc<dyn FileSystemOps>) -> Self {
        Self { fs_ops }
    }
}

impl Default for ConfigureSparseCheckout {
    fn default() -> Self {
        Self::new()
    }
}

impl Task for ConfigureSparseCheckout {
    fn name(&self) -> &'static str {
        "Configure sparse checkout"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        self.fs_ops.exists(&ctx.root().join(".git"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let excluded_files: Vec<String> = ctx.config_read().manifest.excluded_files.clone();

        if excluded_files.is_empty() {
            ctx.log.info("no files to exclude from sparse checkout");
            return Ok(TaskResult::Ok);
        }

        let patterns_str = build_patterns(&excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");
        let previous_patterns = if sparse_file.exists() {
            Some(
                std::fs::read_to_string(&sparse_file)
                    .with_context(|| format!("reading {}", sparse_file.display()))?,
            )
        } else {
            None
        };

        // Check if patterns are already up to date (shared by dry-run and real paths)
        if is_up_to_date(&sparse_file, &patterns_str) {
            ctx.log.info(&format!(
                "already configured ({} files excluded)",
                excluded_files.len()
            ));
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log.dry_run("configure git sparse checkout");
            for file in &excluded_files {
                ctx.log.dry_run(&format!("  exclude: {file}"));
            }
            return Ok(TaskResult::DryRun);
        }

        if worktree_has_local_changes(ctx)? {
            ctx.log
                .warn("local changes detected, skipping sparse checkout reconfiguration");
            return Ok(TaskResult::Skipped("local changes present".to_string()));
        }

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx, &*self.fs_ops);

        let root = ctx.root();

        // Enable non-cone sparse checkout by setting git config directly.
        //
        // Using `git sparse-checkout init --no-cone` is avoided here because it
        // overwrites the sparse-checkout file with default `/*\n!/*/\n` patterns
        // and immediately applies them via an internal `git read-tree`, deleting
        // every repository subdirectory from the working tree.  If the process
        // that invoked this binary inherited a cwd from inside the repository
        // (e.g. a CI script running from `.github/workflows/scripts/`), that
        // directory is deleted and its inode becomes unreachable.  Any child
        // process spawned later (such as `gh copilot plugin list`) inherits the
        // stale cwd and fails with `ENOENT: uv_cwd` when Node.js calls
        // `process.cwd()` during startup.
        //
        // Setting the two config keys directly enables sparse checkout in
        // non-cone mode without modifying the working tree; the subsequent
        // `git read-tree -mu HEAD` then applies only our intentional patterns.
        ctx.log
            .debug("enabling sparse checkout (non-cone mode via git config)");
        ctx.executor
            .run_in(&root, "git", &["config", "core.sparseCheckout", "true"])?;
        ctx.executor.run_in(
            &root,
            "git",
            &["config", "core.sparseCheckoutCone", "false"],
        )?;

        ctx.log.debug(&format!(
            "sparse checkout patterns: 1 inclusion, {} exclusions",
            excluded_files.len()
        ));

        // Write directly to sparse-checkout file
        let info_dir = root.join(".git/info");
        if !info_dir.exists() {
            std::fs::create_dir_all(&info_dir).context("creating .git/info directory")?;
        }
        std::fs::write(&sparse_file, &patterns_str).context("writing sparse-checkout file")?;

        // Reset excluded files to HEAD so read-tree doesn't fail with
        // "not uptodate. Cannot merge." when the working tree is dirty.
        let mut checkout_args = vec!["checkout", "HEAD", "--"];
        let excluded: Vec<&str> = excluded_files
            .iter()
            .filter(|f| root.join(f).exists())
            .map(String::as_str)
            .collect();
        if !excluded.is_empty() {
            checkout_args.extend(&excluded);
            ctx.log.debug(&format!(
                "resetting {} excluded files to HEAD before read-tree",
                excluded.len()
            ));
            // Best-effort: if checkout fails (e.g. file not in HEAD), proceed anyway
            if let Err(e) = ctx.executor.run_in(&root, "git", &checkout_args) {
                ctx.log.debug(&format!("git checkout reset failed: {e}"));
            }
        }

        ctx.log
            .debug("wrote sparse-checkout file, running read-tree");
        if let Err(err) = ctx
            .executor
            .run_in(&root, "git", &["read-tree", "-mu", "HEAD"])
        {
            ctx.log
                .warn("git read-tree failed; restoring previous sparse-checkout configuration");
            restore_sparse_checkout_file(&sparse_file, previous_patterns.as_deref())?;
            ctx.executor
                .run_in(&root, "git", &["read-tree", "-mu", "HEAD"])
                .context("restoring worktree after failed sparse-checkout update")?;
            return Err(err.context("applying sparse-checkout patterns"));
        }

        ctx.log.info(&format!(
            "excluded {} files from checkout",
            excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}

fn worktree_has_local_changes(ctx: &Context) -> Result<bool> {
    let status = ctx.executor.run_in(
        &ctx.root(),
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
    )?;

    Ok(!status.stdout.trim().is_empty())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::{ExecResult, Executor, MockExecutor};
    use crate::fs::MockFileSystemOps;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{empty_config, make_context, make_linux_context};
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
        let patterns = build_patterns(&["symlinks".to_string()]);
        assert_eq!(patterns, "/*\n!/symlinks");
    }

    #[test]
    fn build_patterns_multiple_exclusions() {
        let patterns = build_patterns(&["symlinks".to_string(), "conf".to_string()]);
        assert_eq!(patterns, "/*\n!/symlinks\n!/conf");
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

    // -----------------------------------------------------------------------
    // should_run
    // -----------------------------------------------------------------------

    #[test]
    fn should_run_false_when_git_dir_missing() {
        let config = empty_config(PathBuf::from("/nonexistent/repo"));
        let ctx = make_linux_context(config);
        assert!(!ConfigureSparseCheckout::new().should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_git_dir_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(ConfigureSparseCheckout::new().should_run(&ctx));
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
        // Empty manifest → no exclusions → returns Ok immediately without git calls.
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn run_returns_ok_when_already_up_to_date() {
        let dir = tempfile::tempdir().unwrap();

        // Write the exact patterns that build_patterns would produce
        let info_dir = dir.path().join(".git").join("info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(info_dir.join("sparse-checkout"), "/*\n!/symlinks").unwrap();

        let mut config = empty_config(dir.path().to_path_buf());
        config.manifest.excluded_files.push("symlinks".to_string());
        let ctx = make_linux_context(config);

        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok when sparse-checkout is already up to date, got {result:?}"
        );
    }

    #[test]
    fn run_returns_dry_run_when_patterns_need_update() {
        let dir = tempfile::tempdir().unwrap();
        // No sparse-checkout file → patterns differ → DryRun

        let mut config = empty_config(dir.path().to_path_buf());
        config.manifest.excluded_files.push("symlinks".to_string());
        let mut ctx = make_linux_context(config);
        ctx = ctx.with_dry_run(true);

        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun, got {result:?}"
        );
    }

    #[test]
    fn run_skips_when_worktree_has_local_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let mut config = empty_config(dir.path().to_path_buf());
        config.manifest.excluded_files.push("symlinks".to_string());

        let mut executor = MockExecutor::new();
        executor.expect_run_in().once().returning(|_, _, _| {
            Ok(ExecResult {
                stdout: "M  cli/src/tasks/packages.rs\n".to_string(),
                stderr: String::new(),
                success: true,
                code: Some(0),
            })
        });
        let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes present")),
            "expected local changes skip, got {result:?}"
        );
    }

    #[derive(Debug)]
    struct UntrackedAwareExecutor;

    impl Executor for UntrackedAwareExecutor {
        fn run<'a>(&self, _: &str, _: &'a [&'a str]) -> Result<ExecResult> {
            anyhow::bail!("unexpected run() call")
        }

        fn run_in_with_env<'a>(
            &self,
            _: &std::path::Path,
            _: &str,
            args: &'a [&'a str],
            _: &'a [(&'a str, &'a str)],
        ) -> Result<ExecResult> {
            let stdout = if args.contains(&"--untracked-files=no") {
                String::new()
            } else {
                "?? scratch.txt\n".to_string()
            };

            Ok(ExecResult {
                stdout,
                stderr: String::new(),
                success: true,
                code: Some(0),
            })
        }

        fn run_unchecked<'a>(&self, _: &str, _: &'a [&'a str]) -> Result<ExecResult> {
            anyhow::bail!("unexpected run_unchecked() call")
        }

        fn which(&self, _: &str) -> bool {
            false
        }

        fn which_path(&self, program: &str) -> Result<std::path::PathBuf> {
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
    fn run_writes_sparse_checkout_patterns_and_calls_git() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let mut config = empty_config(dir.path().to_path_buf());
        config.manifest.excluded_files.push("symlinks".to_string());

        // The excluded file "symlinks" does NOT exist on disk, so the
        // `git checkout HEAD -- <files>` step is skipped. Four git calls remain:
        //   1. git status --porcelain --untracked-files=no
        //   2. git config core.sparseCheckout true
        //   3. git config core.sparseCheckoutCone false
        //   4. git read-tree -mu HEAD
        let mut executor = MockExecutor::new();
        executor.expect_run_in().returning(|_, _, _| {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
                code: Some(0),
            })
        });
        let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
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
        //   2. git config core.sparseCheckout → success
        //   3. git config core.sparseCheckoutCone → success
        //   4. git read-tree → fails
        //   5. rollback git read-tree → success
        let mut seq = mockall::Sequence::new();
        let mut executor = MockExecutor::new();
        // git status (clean worktree)
        executor
            .expect_run_in()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            });
        // git config sparseCheckout
        executor
            .expect_run_in()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            });
        // git config sparseCheckoutCone
        executor
            .expect_run_in()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            });
        // git read-tree fails
        executor
            .expect_run_in()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _| anyhow::bail!("mock read-tree failed"));
        // rollback git read-tree
        executor
            .expect_run_in()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            });
        let ctx = make_context(config, Platform::new(Os::Linux, false), Arc::new(executor));

        let err = ConfigureSparseCheckout::new().run(&ctx).unwrap_err();
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
            .withf(|p| p == std::path::Path::new("/home/test/.config/git"))
            .returning(|_| true);
        mock.expect_exists()
            .withf(|p| p == std::path::Path::new("/repo/symlinks/git/gitconfig"))
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
            .withf(|p| p == std::path::Path::new("/home/test/.config/git"))
            .returning(|_| true);
        mock.expect_exists()
            .withf(|p| p == std::path::Path::new("/repo/symlinks/git/gitconfig"))
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
            .withf(|p| p == std::path::Path::new("/home/test/.config/git"))
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
        let ctx = make_linux_context(config);
        let mut mock = MockFileSystemOps::new();
        mock.expect_exists().returning(|_| false);
        let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(mock));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_git_dir_exists_via_mock() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let mut mock = MockFileSystemOps::new();
        mock.expect_exists().returning(|_| true);
        let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(mock));
        assert!(task.should_run(&ctx));
    }
}
