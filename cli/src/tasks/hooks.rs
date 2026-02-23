use anyhow::{Context as _, Result};
use std::any::TypeId;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove};
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
fn discover_hooks(ctx: &Context) -> Result<Vec<HookFileResource>> {
    let hooks_src = ctx.hooks_dir();
    let hooks_dst = ctx.root().join(".git/hooks");

    let mut resources = Vec::new();
    for path in ctx
        .fs_ops
        .read_dir(&hooks_src)
        .with_context(|| format!("reading hooks directory: {}", hooks_src.display()))?
    {
        // Only install executable hook files; skip data files like .ini
        // Skip paths with no filename component (e.g., root or empty paths).
        let Some(file_name) = path.file_name().map(std::path::PathBuf::from) else {
            continue;
        };
        if ctx.fs_ops.is_file(&path) && path.extension().is_none() {
            resources.push(HookFileResource::new(path, hooks_dst.join(file_name)));
        }
    }
    Ok(resources)
}

/// Install git hooks from hooks/ into .git/hooks/.
#[derive(Debug)]
pub struct InstallGitHooks;

impl Task for InstallGitHooks {
    fn name(&self) -> &'static str {
        "Install git hooks"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::reload_config::ReloadConfig>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.fs_ops.exists(&ctx.hooks_dir()) && ctx.fs_ops.exists(&ctx.root().join(".git"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "install hook",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: true,
            },
        )
    }
}

/// Remove git hooks that were installed from hooks/.
#[derive(Debug)]
pub struct UninstallGitHooks;

impl Task for UninstallGitHooks {
    fn name(&self) -> &'static str {
        "Remove git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.fs_ops.exists(&ctx.hooks_dir()) && ctx.fs_ops.exists(&ctx.root().join(".git/hooks"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
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
    use std::sync::Arc;

    // ------------------------------------------------------------------
    // InstallGitHooks::should_run — using MockFileSystemOps
    // ------------------------------------------------------------------

    #[test]
    fn install_should_run_false_when_hooks_dir_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(MockFileSystemOps::new()));
        assert!(!InstallGitHooks.should_run(&ctx));
    }

    #[test]
    fn install_should_run_false_when_git_dir_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        // hooks/ exists but .git/ does not
        let fs = MockFileSystemOps::new().with_existing("/repo/hooks");
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));
        assert!(!InstallGitHooks.should_run(&ctx));
    }

    #[test]
    fn install_should_run_true_when_both_dirs_exist() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_existing("/repo/hooks")
            .with_existing("/repo/.git");
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));
        assert!(InstallGitHooks.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // UninstallGitHooks::should_run — using MockFileSystemOps
    // ------------------------------------------------------------------

    #[test]
    fn uninstall_should_run_false_when_git_hooks_missing() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(MockFileSystemOps::new()));
        assert!(!UninstallGitHooks.should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_both_dirs_exist() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_existing("/repo/hooks")
            .with_existing("/repo/.git/hooks");
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));
        assert!(UninstallGitHooks.should_run(&ctx));
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
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));

        let resources = discover_hooks(&ctx).unwrap();
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
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));

        let resources = discover_hooks(&ctx).unwrap();
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn discover_hooks_targets_point_to_git_hooks_dir() {
        let config = empty_config(PathBuf::from("/repo"));
        let fs = MockFileSystemOps::new()
            .with_dir_entries("/repo/hooks", vec![PathBuf::from("/repo/hooks/pre-commit")])
            .with_file("/repo/hooks/pre-commit");
        let ctx = make_linux_context(config).with_fs_ops(Arc::new(fs));

        let resources = discover_hooks(&ctx).unwrap();
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
        assert!(InstallGitHooks.should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_both_dirs_exist_real_fs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("hooks")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        assert!(UninstallGitHooks.should_run(&ctx));
    }
}
