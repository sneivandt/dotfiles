//! Tasks: install and uninstall Git hooks.
use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove, task_deps,
};
use crate::operations::{FileSystemOps, SystemFileSystemOps};
use crate::resources::hook::HookFileResource;

/// Discover hook file resources from the `hooks/` directory.
///
/// Returns one [`HookFileResource`] per file that has no extension (i.e.
/// conventional hook scripts such as `pre-commit`, `commit-msg`), pairing
/// each source file with its destination path under `.git/hooks/`.
///
/// # Errors
///
/// Returns an error if the `hooks/` directory cannot be read.
fn discover_hooks(ctx: &Context, fs_ops: &dyn FileSystemOps) -> Result<Vec<HookFileResource>> {
    let hooks_src = ctx.hooks_dir();
    let hooks_dst = ctx.root().join(".git/hooks");

    let mut resources = Vec::new();
    for path in fs_ops
        .read_dir(&hooks_src)
        .with_context(|| format!("reading hooks directory: {}", hooks_src.display()))?
    {
        // Only install executable hook files; skip data files like .ini
        // Skip paths with no filename component (e.g., root or empty paths).
        let Some(file_name) = path.file_name().map(std::path::PathBuf::from) else {
            continue;
        };
        if fs_ops.is_file(&path) && path.extension().is_none() {
            resources.push(HookFileResource::new(path, hooks_dst.join(file_name)));
        }
    }
    Ok(resources)
}

/// Install git hooks from hooks/ into .git/hooks/.
#[derive(Debug)]
pub struct InstallGitHooks {
    fs_ops: Arc<dyn FileSystemOps>,
}

impl InstallGitHooks {
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

impl Default for InstallGitHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl Task for InstallGitHooks {
    fn name(&self) -> &'static str {
        "Install git hooks"
    }

    task_deps![super::reload_config::ReloadConfig];

    fn should_run(&self, ctx: &Context) -> bool {
        self.fs_ops.exists(&ctx.hooks_dir()) && self.fs_ops.exists(&ctx.root().join(".git"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx, &*self.fs_ops)?;
        process_resources(ctx, resources, &ProcessOpts::apply_all("install hook"))
    }
}

/// Remove git hooks that were installed from hooks/.
#[derive(Debug)]
pub struct UninstallGitHooks {
    fs_ops: Arc<dyn FileSystemOps>,
}

impl UninstallGitHooks {
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

impl Default for UninstallGitHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl Task for UninstallGitHooks {
    fn name(&self) -> &'static str {
        "Remove git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        self.fs_ops.exists(&ctx.hooks_dir()) && self.fs_ops.exists(&ctx.root().join(".git/hooks"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx, &*self.fs_ops)?;
        process_resources_remove(ctx, resources, "remove hook")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::operations::MockFileSystemOps;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // InstallGitHooks::should_run — using MockFileSystemOps
    // ------------------------------------------------------------------

    #[test]
    fn install_should_run_false_when_hooks_dir_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let task = InstallGitHooks::with_fs_ops(Arc::new(MockFileSystemOps::new()));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn install_should_run_false_when_git_dir_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        // hooks/ exists but .git/ does not
        let fs = MockFileSystemOps::new().with_existing("/repo/hooks");
        let ctx = make_linux_context(config);
        let task = InstallGitHooks::with_fs_ops(Arc::new(fs));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn install_should_run_true_when_both_dirs_exist() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_existing("/repo/hooks")
            .with_existing("/repo/.git");
        let ctx = make_linux_context(config);
        let task = InstallGitHooks::with_fs_ops(Arc::new(fs));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // UninstallGitHooks::should_run — using MockFileSystemOps
    // ------------------------------------------------------------------

    #[test]
    fn uninstall_should_run_false_when_git_hooks_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        let task = UninstallGitHooks::with_fs_ops(Arc::new(MockFileSystemOps::new()));
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_both_dirs_exist() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_existing("/repo/hooks")
            .with_existing("/repo/.git/hooks");
        let ctx = make_linux_context(config);
        let task = UninstallGitHooks::with_fs_ops(Arc::new(fs));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // discover_hooks — using MockFileSystemOps
    // ------------------------------------------------------------------

    #[test]
    fn discover_hooks_returns_hook_files_without_extension() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_dir_entries(
                "/repo/hooks",
                vec![
                    PathBuf::from("/repo/hooks/pre-commit"),
                    PathBuf::from("/repo/hooks/commit-msg"),
                    PathBuf::from("/repo/hooks/hooks.ini"), // should be skipped (has extension)
                ],
            )
            .with_file("/repo/hooks/pre-commit")
            .with_file("/repo/hooks/commit-msg")
            .with_file("/repo/hooks/hooks.ini");
        let ctx = make_linux_context(config);

        let resources = discover_hooks(&ctx, &fs).unwrap();
        assert_eq!(resources.len(), 2);
        let names: Vec<_> = resources
            .iter()
            .map(|r| r.source.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"pre-commit".to_string()));
        assert!(names.contains(&"commit-msg".to_string()));
    }

    #[test]
    fn discover_hooks_skips_directories() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_dir_entries(
                "/repo/hooks",
                vec![
                    PathBuf::from("/repo/hooks/pre-commit"),
                    PathBuf::from("/repo/hooks/subdir"), // directory, not a file
                ],
            )
            .with_file("/repo/hooks/pre-commit");
        // /repo/hooks/subdir is NOT added as a file → is_file returns false for it
        let ctx = make_linux_context(config);

        let resources = discover_hooks(&ctx, &fs).unwrap();
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn discover_hooks_targets_point_to_git_hooks_dir() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_dir_entries("/repo/hooks", vec![PathBuf::from("/repo/hooks/pre-commit")])
            .with_file("/repo/hooks/pre-commit");
        let ctx = make_linux_context(config);

        let resources = discover_hooks(&ctx, &fs).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].target,
            PathBuf::from("/repo/.git/hooks/pre-commit")
        );
    }

    // ------------------------------------------------------------------
    // Real-filesystem tests (kept for integration coverage)
    // ------------------------------------------------------------------

    #[test]
    fn install_should_run_true_when_both_dirs_exist_real_fs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("hooks")).unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(InstallGitHooks::new().should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_both_dirs_exist_real_fs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("hooks")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(UninstallGitHooks::new().should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // InstallGitHooks::run / UninstallGitHooks::run — real filesystem
    // ------------------------------------------------------------------

    #[test]
    fn install_run_installs_hook_into_git_hooks() {
        let dir = tempfile::tempdir().unwrap();

        // Create hooks/ dir with a hook file (no extension)
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("pre-commit"), "#!/bin/sh\nexit 0").unwrap();

        // Create .git/hooks/ dir
        let git_hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&git_hooks_dir).unwrap();

        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        let task = InstallGitHooks::new();

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(
            git_hooks_dir.join("pre-commit").exists(),
            "pre-commit hook should be installed in .git/hooks/"
        );
    }

    #[test]
    fn uninstall_run_removes_installed_hook() {
        let dir = tempfile::tempdir().unwrap();

        // Create hooks/ dir with a hook file
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("pre-commit"), "#!/bin/sh\nexit 0").unwrap();

        // Create .git/hooks/ with the hook already installed
        let git_hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&git_hooks_dir).unwrap();
        std::fs::write(git_hooks_dir.join("pre-commit"), "#!/bin/sh\nexit 0").unwrap();

        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        let task = UninstallGitHooks::new();

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(
            !git_hooks_dir.join("pre-commit").exists(),
            "pre-commit hook should be removed from .git/hooks/"
        );
    }
}
