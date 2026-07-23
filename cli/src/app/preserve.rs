//! Preserve managed links before sparse checkout removes their sources.

use anyhow::Result;

use crate::domains::files::config::symlinks::{self, Symlink};
use crate::domains::files::resources::symlink::SymlinkResource;
use crate::domains::files::symlinks::build_resources;
use crate::domains::repository::config::manifest::Manifest;
use crate::engine::{
    Context, IntrinsicState, ResourceState, Task, TaskResult, process_resources_remove,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Preserve managed files that are about to leave the sparse checkout.
#[derive(Debug)]
pub struct MaterializeExcludedSymlinks {
    all_symlinks: ConfigHandle<Vec<Symlink>>,
    manifest: ConfigHandle<Manifest>,
}

impl MaterializeExcludedSymlinks {
    /// Create the task from the complete main-repository symlink set and the
    /// active sparse-checkout manifest.
    #[must_use]
    pub const fn new(
        all_symlinks: ConfigHandle<Vec<Symlink>>,
        manifest: ConfigHandle<Manifest>,
    ) -> Self {
        Self {
            all_symlinks,
            manifest,
        }
    }

    fn managed_excluded_resources(&self, ctx: &Context) -> Result<Vec<SymlinkResource>> {
        let manifest = self.manifest.read();
        let symlinks = self.all_symlinks.read();
        let excluded: Vec<Symlink> = symlinks
            .iter()
            .filter(|symlink| manifest.excludes_source(&symlink.source))
            .cloned()
            .collect();
        drop(symlinks);
        drop(manifest);

        let expanded = symlinks::expand_present_glob_patterns(&excluded, ctx.root())?;
        build_resources(ctx, &expanded)
            .into_iter()
            .filter_map(|resource| match resource.current_state() {
                Ok(ResourceState::Correct) => Some(Ok(resource)),
                Ok(
                    ResourceState::Missing
                    | ResourceState::Incorrect { .. }
                    | ResourceState::Invalid { .. }
                    | ResourceState::Unknown { .. },
                ) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
}

impl Task for MaterializeExcludedSymlinks {
    task_metadata! {
        name: "Materialize excluded symlinks",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.manifest.read().excluded_files.is_empty()
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        let resources = self.managed_excluded_resources(ctx)?;
        if resources.is_empty() {
            return Ok(None);
        }
        ctx.log().task_stage(self.name());
        process_resources_remove(ctx, resources, "materialize").map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources_remove(ctx, self.managed_excluded_resources(ctx)?, "materialize")
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    fn symlink(source: &str, root: &std::path::Path) -> Symlink {
        Symlink {
            source: source.to_string(),
            target: None,
            origin: Some(root.to_path_buf()),
        }
    }

    fn task(root: &std::path::Path, excluded_files: Vec<String>) -> MaterializeExcludedSymlinks {
        MaterializeExcludedSymlinks::new(
            ConfigHandle::new(vec![symlink("config/i3/config", root)]),
            ConfigHandle::new(Manifest { excluded_files }),
        )
    }

    #[cfg(unix)]
    #[test]
    fn run_materializes_managed_excluded_symlink() {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let source = repo.path().join("symlinks/config/i3/config");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "i3 config").unwrap();
        let target = home.path().join(".config/i3/config");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();
        let ctx = make_linux_context(empty_config(repo.path().to_path_buf()))
            .with_home(home.path().to_path_buf());

        let result = task(repo.path(), vec!["config/i3/".to_string()])
            .run(&ctx)
            .unwrap();

        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed == 1
        ));
        assert!(!std::fs::symlink_metadata(&target).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "i3 config");
    }

    #[cfg(unix)]
    #[test]
    fn run_leaves_non_excluded_symlink_managed() {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let source = repo.path().join("symlinks/config/i3/config");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "i3 config").unwrap();
        let target = home.path().join(".config/i3/config");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();
        let ctx = make_linux_context(empty_config(repo.path().to_path_buf()))
            .with_home(home.path().to_path_buf());

        let result = task(repo.path(), vec!["config/Code/".to_string()])
            .run(&ctx)
            .unwrap();

        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed == 0 && stats.failed == 0
        ));
        assert!(std::fs::symlink_metadata(&target).unwrap().is_symlink());
    }

    #[test]
    fn should_not_run_without_exclusions() {
        let root = PathBuf::from("/repo");
        let ctx = make_linux_context(empty_config(root.clone()));
        assert!(!task(&root, vec![]).should_run(&ctx));
    }
}
