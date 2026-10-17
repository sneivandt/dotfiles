//! Tasks: install and uninstall Git hooks.
use anyhow::{Context as _, Result};

use crate::domains::git::resources::hook::HookFileResource;
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove,
    task_metadata,
};

/// Discover hook file resources from the `hooks/` directory.
///
/// Returns one [`HookFileResource`] per file that has no extension (i.e.
/// conventional hook scripts such as `pre-commit`, `commit-msg`), pairing
/// each source file with its destination in Git's configured hooks directory.
///
/// # Errors
///
/// Returns an error if the `hooks/` directory cannot be read.
fn discover_hooks(ctx: &Context) -> Result<Vec<HookFileResource>> {
    let hooks_src = ctx.hooks_dir().to_path_buf();
    let repository =
        git2::Repository::open(ctx.root()).context("opening repository for Git hooks")?;
    let hooks_dst = match repository.config()?.get_path("core.hooksPath") {
        Ok(path) => ctx.root().join(path),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            repository.commondir().join("hooks")
        }
        Err(error) => return Err(error).context("reading core.hooksPath"),
    };

    let mut resources = Vec::new();
    for entry in std::fs::read_dir(&hooks_src)
        .with_context(|| format!("reading hooks directory: {}", hooks_src.display()))?
    {
        let entry =
            entry.with_context(|| format!("reading hooks directory: {}", hooks_src.display()))?;
        let path = entry.path();
        // Extensionless files are hook entry points; helpers and directories stay out.
        if path.is_file() && path.extension().is_none() {
            let resource = HookFileResource::new(path, hooks_dst.join(entry.file_name()));
            // A user-owned hooksPath can execute the tracked sources directly.
            if !resource.targets_source()? {
                resources.push(resource);
            }
        }
    }
    Ok(resources)
}

/// Install git hooks from hooks/ into .git/hooks/.
#[derive(Debug, Default)]
pub struct InstallGitHooks;

impl InstallGitHooks {
    /// Create a Git-hook task.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Task for InstallGitHooks {
    task_metadata! {
        name: "Git hooks",
        selector: "git-hooks",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources(ctx, resources, &ProcessOpts::strict("install"))
    }
}

/// Remove git hooks that were installed from hooks/.
#[derive(Debug, Default)]
pub struct UninstallGitHooks;

impl UninstallGitHooks {
    /// Create a Git-hook task.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Task for UninstallGitHooks {
    task_metadata! {
        name: "Git hooks",
        selector: "git-hooks",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources_remove(ctx, resources, "remove")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_linux_context;

    fn tasks() -> [&'static dyn Task; 2] {
        [&InstallGitHooks, &UninstallGitHooks]
    }

    #[test]
    fn applicability_requires_hooks_and_git_paths_for_both_actions() {
        for hooks in [false, true] {
            for git in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                if hooks {
                    std::fs::create_dir(dir.path().join("hooks")).unwrap();
                }
                if git {
                    std::fs::create_dir(dir.path().join(".git")).unwrap();
                }
                let ctx = make_linux_context(dir.path().to_path_buf(), None);
                for task in tasks() {
                    assert_eq!(
                        task.should_run(&ctx),
                        hooks && git,
                        "{}: hooks={hooks}, git={git}",
                        task.name()
                    );
                    assert!(task.dependencies().is_empty());
                }
            }
        }
    }

    #[test]
    fn discovery_filters_files_and_resolves_the_git_destination() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        for name in ["pre-commit", "commit-msg", "hooks.ini"] {
            std::fs::write(hooks.join(name), "hook").unwrap();
        }
        std::fs::create_dir(hooks.join("subdir")).unwrap();
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        let resources = discover_hooks(&ctx).unwrap();
        let mut names: Vec<_> = resources
            .iter()
            .map(|resource| {
                assert_eq!(resource.source.parent(), Some(hooks.as_path()));
                assert_eq!(
                    resource.target.parent().unwrap().canonicalize().unwrap(),
                    dir.path().join(".git/hooks").canonicalize().unwrap()
                );
                assert_eq!(resource.source.file_name(), resource.target.file_name());
                resource.source.file_name().unwrap().to_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, ["commit-msg", "pre-commit"]);
    }

    #[test]
    fn discovery_propagates_directory_read_errors_for_both_actions() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("hooks"), "not a directory").unwrap();
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        for task in tasks() {
            let error = task.run(&ctx).unwrap_err();
            assert!(error.to_string().contains("reading hooks directory"));
        }
    }

    #[test]
    fn install_run_installs_hook_into_git_hooks() {
        let dir = tempfile::tempdir().unwrap();

        // Create hooks/ dir with a hook file (no extension)
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("pre-commit"), "#!/bin/sh\nexit 0").unwrap();

        // Create .git/hooks/ dir
        let git_hooks_dir = dir.path().join(".git").join("hooks");
        git2::Repository::init(dir.path()).unwrap();

        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        let task = InstallGitHooks::new();

        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 1
        ));
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

        // Create .git/hooks/ with the hook already installed (executable, as apply() would)
        let git_hooks_dir = dir.path().join(".git").join("hooks");
        git2::Repository::init(dir.path()).unwrap();
        std::fs::write(git_hooks_dir.join("pre-commit"), "#!/bin/sh\nexit 0").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                git_hooks_dir.join("pre-commit"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        let task = UninstallGitHooks::new();

        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 1
        ));
        assert!(
            !git_hooks_dir.join("pre-commit").exists(),
            "pre-commit hook should be removed from .git/hooks/"
        );
    }

    #[test]
    fn linked_worktree_hooks_use_git_directory_and_custom_hooks_path() {
        for custom in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let repository = git2::Repository::init(dir.path()).unwrap();
            std::fs::create_dir(dir.path().join("hooks")).unwrap();
            std::fs::write(dir.path().join("hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
            let mut index = repository.index().unwrap();
            index
                .add_path(std::path::Path::new("hooks/pre-commit"))
                .unwrap();
            let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
            let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            repository
                .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
                .unwrap();
            let linked = dir.path().join("linked");
            repository
                .worktree("fixture-worktree", &linked, None)
                .unwrap();
            if custom {
                repository
                    .config()
                    .unwrap()
                    .set_str("core.hooksPath", "custom-hooks")
                    .unwrap();
            }
            let target = if custom {
                linked.join("custom-hooks/pre-commit")
            } else {
                repository.path().join("hooks/pre-commit")
            };
            let ctx = make_linux_context(linked.clone(), None);
            assert!(linked.join(".git").is_file());
            assert!(InstallGitHooks::new().should_run(&ctx));
            crate::test_helpers::assert_task_changed(
                &InstallGitHooks::new().run(&ctx.with_dry_run(true)).unwrap(),
            );
            assert!(!target.exists());
            crate::test_helpers::assert_task_changed(&InstallGitHooks::new().run(&ctx).unwrap());
            assert!(target.is_file());
            let current = InstallGitHooks::new().run(&ctx).unwrap();
            assert!(
                matches!(current, TaskResult::Batch(stats) if stats.changed_count() == 0 && stats.already_ok_count() == 1)
            );
            assert!(UninstallGitHooks::new().should_run(&ctx));
            crate::test_helpers::assert_task_changed(&UninstallGitHooks::new().run(&ctx).unwrap());
            assert!(!target.exists());
        }
    }
}
