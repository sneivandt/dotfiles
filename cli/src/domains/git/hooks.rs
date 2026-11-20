//! Tasks: install and uninstall Git hooks.
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::domains::git::resources::hook::{
    HookFileResource, OWNERSHIP_EXTENSION, REMOVAL_EXTENSION,
};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove,
    task_metadata,
};

/// Discover current sources and, for removal, installed copies of retired hooks.
fn discover_hooks(ctx: &Context, installing: bool) -> Result<Vec<HookFileResource>> {
    let hooks_src = ctx.hooks_dir();
    let repository =
        git2::Repository::open(ctx.root()).context("opening repository for Git hooks")?;
    let config = repository.config()?;
    let hooks_dst = match config.get_path("core.hooksPath") {
        Ok(path) => {
            let entry = config.get_entry("core.hooksPath")?;
            anyhow::ensure!(
                matches!(
                    entry.level(),
                    git2::ConfigLevel::Local | git2::ConfigLevel::Worktree
                ),
                "core.hooksPath is inherited from shared Git configuration; set a repository-local hooksPath before installing dotfiles hooks"
            );
            ctx.root().join(path)
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            repository.commondir().join("hooks")
        }
        Err(error) => return Err(error).context("reading core.hooksPath"),
    };

    let mut names = BTreeSet::new();
    for entry in read_hooks_directory(hooks_src, !installing)? {
        let entry = entry?;
        let path = entry.path();
        // Extensionless files are hook entry points; helpers and directories stay out.
        if path.extension().is_none() {
            let metadata = std::fs::metadata(&path)
                .with_context(|| format!("inspecting hook source {}", path.display()))?;
            if metadata.is_file() {
                names.insert(entry.file_name());
            } else if !metadata.is_dir() {
                anyhow::bail!("hook source is not a regular file: {}", path.display());
            }
        }
    }
    if !installing {
        add_recorded_hooks(&hooks_dst, &mut names)?;
    }

    let mut resources = Vec::new();
    for name in names {
        let resource = HookFileResource::new(hooks_src.join(&name), hooks_dst.join(&name));
        // A user-owned hooksPath can execute the tracked sources directly.
        if resource.targets_source()? {
            if installing {
                check_direct_hook(&resource)?;
            }
        } else {
            resources.push(resource);
        }
    }
    Ok(resources)
}

fn read_hooks_directory(
    path: &Path,
    allow_missing: bool,
) -> Result<impl Iterator<Item = std::io::Result<std::fs::DirEntry>>> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => Some(entries),
        Err(error) if allow_missing && error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading hooks directory: {}", path.display()));
        }
    };
    Ok(entries.into_iter().flatten())
}

fn add_recorded_hooks(directory: &Path, names: &mut BTreeSet<OsString>) -> Result<()> {
    for entry in read_hooks_directory(directory, true)? {
        let entry = entry?;
        let name = entry.file_name();
        let record = Path::new(&name);
        if let Some(ext) = record.extension()
            && (ext == REMOVAL_EXTENSION
                || (ext == OWNERSHIP_EXTENSION && entry.file_type()?.is_file()))
            && let Some(stem) = record.file_stem()
            && Path::new(stem).file_name() == Some(stem)
            && Path::new(stem).extension().is_none()
        {
            names.insert(stem.to_owned());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn check_direct_hook(resource: &HookFileResource) -> Result<()> {
    nix::unistd::access(&resource.target, nix::unistd::AccessFlags::X_OK).with_context(|| {
        format!(
            "Git cannot execute its hook at {}; make it executable (chmod +x) or configure a separate repository-local core.hooksPath",
            resource.target.display()
        )
    })
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "match the Unix hook check signature"
)]
const fn check_direct_hook(_resource: &HookFileResource) -> Result<()> {
    Ok(())
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
        let resources = discover_hooks(ctx, true)?;
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
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx, false)?;
        process_resources_remove(ctx, resources, "remove")
    }
}

#[cfg(all(test, unix))]
#[path = "tests/template_hooks.rs"]
mod template_hooks;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_linux_context;

    fn init_repo(root: &Path) -> git2::Repository {
        let repository = git2::Repository::init(root).unwrap();
        repository
            .config()
            .unwrap()
            .set_str(
                "core.hooksPath",
                repository.path().join("hooks").to_str().unwrap(),
            )
            .unwrap();
        repository
    }

    fn tasks() -> [&'static dyn Task; 2] {
        [&InstallGitHooks, &UninstallGitHooks]
    }

    #[test]
    fn only_installation_requires_the_source_directory() {
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
                assert_eq!(InstallGitHooks.should_run(&ctx), hooks && git);
                assert_eq!(UninstallGitHooks.should_run(&ctx), git);
                for task in tasks() {
                    assert!(task.dependencies().is_empty());
                }
            }
        }
    }

    #[test]
    fn discovery_filters_files_and_resolves_the_git_destination() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        for name in ["pre-commit", "commit-msg", "hooks.ini"] {
            std::fs::write(hooks.join(name), "hook").unwrap();
        }
        std::fs::create_dir(hooks.join("subdir")).unwrap();
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        let resources = discover_hooks(&ctx, true).unwrap();
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
        init_repo(dir.path());
        std::fs::write(dir.path().join("hooks"), "not a directory").unwrap();
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        for task in tasks() {
            let error = task.run(&ctx).unwrap_err();
            assert!(error.to_string().contains("reading hooks directory"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn broken_hook_source_aborts_discovery_before_any_publication() {
        let dir = tempfile::tempdir().unwrap();
        let repository = init_repo(dir.path());
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), "new").unwrap();
        std::os::unix::fs::symlink("missing", hooks.join("commit-msg")).unwrap();
        let target = repository.path().join("hooks/pre-commit");
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        assert!(
            InstallGitHooks
                .run(&ctx)
                .unwrap_err()
                .to_string()
                .contains("inspecting hook source")
        );
        assert!(!target.exists());
        std::fs::write(hooks.join("missing"), "recovered").unwrap();
        assert!(InstallGitHooks.run(&ctx).is_ok());
        assert_eq!(std::fs::read(target).unwrap(), b"new");
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
        init_repo(dir.path());

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
        init_repo(dir.path());
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
    fn uninstall_removes_retired_owned_hooks_and_preserves_other_files() {
        for remove_source_directory in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            init_repo(dir.path());
            let source = dir.path().join("hooks");
            let destination = dir.path().join(".git/hooks");
            std::fs::create_dir(&source).unwrap();
            for name in ["pre-commit", "commit-msg"] {
                std::fs::write(source.join(name), "#!/bin/sh\nexit 0\n").unwrap();
            }
            let ctx = make_linux_context(dir.path().to_path_buf(), None);
            crate::test_helpers::assert_task_changed(&InstallGitHooks.run(&ctx).unwrap());

            for name in ["pre-commit", "commit-msg"] {
                std::fs::remove_file(source.join(name)).unwrap();
            }
            if remove_source_directory {
                std::fs::remove_dir(&source).unwrap();
            }
            let retired = destination.join("pre-commit");
            let record = destination.join("pre-commit.dotfiles-owned");
            let modified = destination.join("commit-msg");
            let modified_record = destination.join("commit-msg.dotfiles-owned");
            let foreign = destination.join("post-commit");
            std::fs::write(&modified, "user edit").unwrap();
            std::fs::write(&foreign, "foreign hook").unwrap();

            assert!(UninstallGitHooks.should_run(&ctx));
            let preview = UninstallGitHooks.run(&ctx.with_dry_run(true)).unwrap();
            assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 1);
            assert!(retired.is_file() && record.is_file());

            let result = UninstallGitHooks.run(&ctx).unwrap();
            assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
            assert!(!retired.exists() && !record.exists());
            assert_eq!(std::fs::read_to_string(&modified).unwrap(), "user edit");
            assert!(modified_record.is_file());
            assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "foreign hook");

            let retry = UninstallGitHooks.run(&ctx).unwrap();
            assert_eq!(crate::test_helpers::task_batch(&retry).changed_count(), 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn direct_source_hook_requires_execute_access_and_is_preserved_on_removal() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let repository = init_repo(dir.path());
        repository
            .config()
            .unwrap()
            .set_str("core.hooksPath", "hooks")
            .unwrap();
        std::fs::create_dir(dir.path().join("hooks")).unwrap();
        let path = dir.path().join("hooks/pre-commit");
        let contents = "#!/bin/sh\nexit 0\n";
        std::fs::write(&path, contents).unwrap();
        let ctx = make_linux_context(dir.path().to_path_buf(), None);
        for mode in [0o644, 0o755] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            for dry_run in [false, true] {
                let result = InstallGitHooks.run(&ctx.with_dry_run(dry_run));
                if mode == 0o644 {
                    let error = result.unwrap_err().to_string();
                    assert!(
                        error.contains("hooks/pre-commit") && error.contains("chmod +x"),
                        "{error}"
                    );
                } else {
                    crate::test_helpers::assert_task_ok(&result.unwrap());
                }
            }
            let removal = UninstallGitHooks.run(&ctx).unwrap();
            let stats = crate::test_helpers::task_batch(&removal);
            assert_eq!(stats.changed_count(), 0);
            assert_eq!(stats.failed_count(), 0);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
    }

    #[test]
    fn linked_worktree_hooks_use_git_directory_and_custom_hooks_path() {
        for custom in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let repository = init_repo(dir.path());
            std::fs::create_dir(dir.path().join("hooks")).unwrap();
            std::fs::write(dir.path().join("hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
            let mut index = repository.index().unwrap();
            index.add_path(Path::new("hooks/pre-commit")).unwrap();
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
