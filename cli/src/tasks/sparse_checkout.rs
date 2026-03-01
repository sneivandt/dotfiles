//! Task: configure sparse checkout.
use anyhow::{Context as _, Result};
use std::path::Path;
use std::sync::Arc;

use super::{Context, Task, TaskResult};
use crate::operations::{FileSystemOps, SystemFileSystemOps};

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
    let Ok(target) = fs.read_link(path) else {
        return false;
    };
    // Resolve relative symlink targets relative to the symlink's directory
    let resolved_target = if target.is_absolute() {
        target
    } else {
        path.parent()
            .map_or_else(|| target.clone(), |parent| parent.join(&target))
    };
    resolved_target.starts_with(dir) && !fs.exists(&resolved_target)
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

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx, &*self.fs_ops);

        let root = ctx.root();

        // Enable sparse checkout with non-cone mode for full pattern matching.
        // Non-cone mode supports negation patterns (e.g., !/<file>) which are
        // needed to selectively exclude files.
        ctx.log
            .debug("initializing sparse checkout (non-cone mode)");
        ctx.executor
            .run_in(&root, "git", &["sparse-checkout", "init", "--no-cone"])?;

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
        ctx.executor
            .run_in(&root, "git", &["read-tree", "-mu", "HEAD"])?;

        ctx.log.info(&format!(
            "excluded {} files from checkout",
            excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::operations::{MockFileSystemOps, SystemFileSystemOps};
    use crate::platform::{Os, Platform};
    use crate::resources::test_helpers::MockExecutor;
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
        ctx.dry_run = true;

        let result = ConfigureSparseCheckout::new().run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun, got {result:?}"
        );
    }

    #[test]
    fn run_writes_sparse_checkout_patterns_and_calls_git() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let mut config = empty_config(dir.path().to_path_buf());
        config.manifest.excluded_files.push("symlinks".to_string());

        // The excluded file "symlinks" does NOT exist on disk, so the
        // `git checkout HEAD -- <files>` step is skipped.  Two git calls remain:
        //   1. git sparse-checkout init --no-cone
        //   2. git read-tree -mu HEAD
        let executor = MockExecutor::with_responses(vec![
            (true, String::new()), // sparse-checkout init
            (true, String::new()), // read-tree
        ]);
        let ctx = make_context(
            config,
            Arc::new(Platform::new(Os::Linux, false)),
            Arc::new(executor),
        );

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

    // -----------------------------------------------------------------------
    // remove_broken_git_symlinks — using MockFileSystemOps
    // -----------------------------------------------------------------------

    #[test]
    fn remove_broken_git_symlinks_skips_when_git_config_dir_missing() {
        // ~/.config/git does not exist → function should return immediately
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let fs = Arc::new(MockFileSystemOps::new());
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
        let fs = Arc::new(
            MockFileSystemOps::new()
                .with_dir_entries("/home/test/.config/git", vec![symlink_path.clone()])
                .with_symlink(symlink_path.clone(), target.clone())
                .with_existing(target), // target exists → not broken
        );
        remove_broken_git_symlinks(&ctx, &*fs);
        assert!(
            fs.exists(&symlink_path),
            "valid symlink must not be removed"
        );
    }

    #[test]
    fn remove_broken_git_symlinks_removes_dangling_symlinks_pointing_into_symlinks_dir() {
        // Symlink whose target is under symlinks_dir and does not exist → must be removed
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let symlink_path = PathBuf::from("/home/test/.config/git/gitconfig");
        let target = PathBuf::from("/repo/symlinks/git/gitconfig");
        let fs = Arc::new(
            MockFileSystemOps::new()
                .with_dir_entries("/home/test/.config/git", vec![symlink_path.clone()])
                .with_symlink(symlink_path.clone(), target), // target not in existing → broken
        );
        remove_broken_git_symlinks(&ctx, &*fs);
        assert!(
            !fs.exists(&symlink_path),
            "dangling symlink must be removed"
        );
    }

    #[test]
    fn remove_broken_git_symlinks_ignores_regular_files() {
        // A regular file (not a symlink) must never be removed
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let file_path = PathBuf::from("/home/test/.config/git/config");
        let fs = Arc::new(
            MockFileSystemOps::new()
                .with_dir_entries("/home/test/.config/git", vec![file_path.clone()])
                .with_file(file_path.clone()),
        );
        remove_broken_git_symlinks(&ctx, &*fs);
        assert!(fs.exists(&file_path), "regular file must not be removed");
    }

    // -----------------------------------------------------------------------
    // should_run — using MockFileSystemOps
    // -----------------------------------------------------------------------

    #[test]
    fn should_run_false_when_git_dir_missing_via_mock() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let fs = MockFileSystemOps::new(); // no .git registered
        let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(fs));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_git_dir_exists_via_mock() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let fs = MockFileSystemOps::new().with_existing("/repo/.git");
        let task = ConfigureSparseCheckout::with_fs_ops(Arc::new(fs));
        assert!(task.should_run(&ctx));
    }
}
