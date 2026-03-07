//! Tasks: install and uninstall symlinks.
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources_remove, resource_task};
use crate::resources::symlink::SymlinkResource;

/// Build a single [`SymlinkResource`] from a config entry.
fn build_resource(
    s: &crate::config::symlinks::Symlink,
    symlinks_dir: &Path,
    home: &Path,
    executor: &Arc<dyn crate::exec::Executor>,
) -> SymlinkResource {
    let target = s.target.as_ref().map_or_else(
        || compute_target(home, &s.source),
        |explicit| home.join(explicit),
    );
    SymlinkResource::new(symlinks_dir.join(&s.source), target, Arc::clone(executor))
}

/// Build [`SymlinkResource`] instances from the loaded config.
fn build_resources(ctx: &Context) -> Vec<SymlinkResource> {
    let symlinks_dir = ctx.symlinks_dir();
    ctx.config_read()
        .symlinks
        .iter()
        .map(|s| build_resource(s, &symlinks_dir, &ctx.home, &ctx.executor))
        .collect()
}

resource_task! {
    /// Create symlinks from symlinks/ to $HOME.
    pub InstallSymlinks {
        name: "Install symlinks",
        deps: [
            super::reload_config::ReloadConfig,
            super::developer_mode::EnableDeveloperMode,
        ],
        items: |ctx| ctx.config_read().symlinks.clone(),
        build: |s, ctx| {
            let symlinks_dir = ctx.symlinks_dir();
            build_resource(&s, &symlinks_dir, &ctx.home, &ctx.executor)
        },
        opts: ProcessOpts::strict("link"),
    }
}

/// Remove installed symlinks.
#[derive(Debug)]
pub struct UninstallSymlinks;

impl Task for UninstallSymlinks {
    fn name(&self) -> &'static str {
        "Remove symlinks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources_remove(ctx, build_resources(ctx), "unlink")
    }
}

/// Compute the default target path in $HOME for a symlink source.
///
/// Symlink sources like "bashrc" map to "$HOME/.bashrc".
/// Sources like "config/git/config" map to "$HOME/.config/git/config".
///
/// When a non-standard target path is required (e.g. Windows paths under
/// `AppData/` or `Documents/`), use an explicit `target` field in
/// `conf/symlinks.toml` rather than relying on naming conventions.
fn compute_target(home: &Path, source: &str) -> std::path::PathBuf {
    home.join(format!(".{source}"))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::symlinks::Symlink;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // compute_target
    // ------------------------------------------------------------------

    #[test]
    fn target_for_bashrc() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "bashrc");
        assert_eq!(target, PathBuf::from("/home/user/.bashrc"));
    }

    #[test]
    fn target_for_config_subpath() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "config/git/config");
        assert_eq!(target, PathBuf::from("/home/user/.config/git/config"));
    }

    #[test]
    fn target_for_ssh() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "ssh/config");
        assert_eq!(target, PathBuf::from("/home/user/.ssh/config"));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::should_run
    // ------------------------------------------------------------------

    #[test]
    fn install_should_run_is_true_without_explicit_guard() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(InstallSymlinks.should_run(&ctx));
    }

    #[test]
    fn install_should_run_true_when_symlinks_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.symlinks.push(Symlink {
            source: "bashrc".to_string(),
            target: None,
        });
        let ctx = make_linux_context(config);
        assert!(InstallSymlinks.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // UninstallSymlinks::should_run
    // ------------------------------------------------------------------

    #[test]
    fn uninstall_should_run_false_when_no_symlinks_configured() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!UninstallSymlinks.should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_symlinks_configured() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.symlinks.push(Symlink {
            source: "bashrc".to_string(),
            target: None,
        });
        let ctx = make_linux_context(config);
        assert!(UninstallSymlinks.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::run / UninstallSymlinks::run — real filesystem
    // ------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn install_run_creates_symlink_for_configured_source() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        // Create symlinks/ dir and a source file inside it
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

        let mut config = empty_config(repo_dir.path().to_path_buf());
        config.symlinks.push(Symlink {
            source: "bashrc".to_string(),
            target: None,
        });
        let ctx = make_linux_context(config);
        let ctx = ctx.with_home(home_dir.path().to_path_buf());

        let result = InstallSymlinks.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        // Symlink must exist at $HOME/.bashrc and point to symlinks/bashrc
        let link = home_dir.path().join(".bashrc");
        assert!(
            link.symlink_metadata().is_ok(),
            "symlink should exist at ~/.bashrc"
        );
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, symlinks_dir.join("bashrc"));
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_run_materializes_symlink_content() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        // Create source file
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

        // Pre-create the symlink at $HOME/.bashrc
        let link = home_dir.path().join(".bashrc");
        std::os::unix::fs::symlink(symlinks_dir.join("bashrc"), &link).unwrap();

        let mut config = empty_config(repo_dir.path().to_path_buf());
        config.symlinks.push(Symlink {
            source: "bashrc".to_string(),
            target: None,
        });
        let ctx = make_linux_context(config);
        let ctx = ctx.with_home(home_dir.path().to_path_buf());

        let result = UninstallSymlinks.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        // Must be a regular file (materialized), not a symlink
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should be materialized to a regular file"
        );
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "# bash config");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_run_parallel_materializes_similar_file_names() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        let symlinks_dir = repo_dir.path().join("symlinks/config/systemd/user");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(
            symlinks_dir.join("clean-home-tmp.service"),
            "[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();
        std::fs::write(
            symlinks_dir.join("clean-home-tmp.timer"),
            "[Timer]\nOnCalendar=daily\n",
        )
        .unwrap();

        let target_dir = home_dir.path().join(".config/systemd/user");
        std::fs::create_dir_all(&target_dir).unwrap();
        let service_link = target_dir.join("clean-home-tmp.service");
        let timer_link = target_dir.join("clean-home-tmp.timer");
        std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.service"), &service_link)
            .unwrap();
        std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.timer"), &timer_link).unwrap();

        let mut config = empty_config(repo_dir.path().to_path_buf());
        config.symlinks.push(Symlink {
            source: "config/systemd/user/clean-home-tmp.service".to_string(),
            target: None,
        });
        config.symlinks.push(Symlink {
            source: "config/systemd/user/clean-home-tmp.timer".to_string(),
            target: None,
        });
        let ctx = make_linux_context(config)
            .with_home(home_dir.path().to_path_buf())
            .with_parallel(true);

        let result = UninstallSymlinks.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        let service_meta = std::fs::symlink_metadata(&service_link).unwrap();
        let timer_meta = std::fs::symlink_metadata(&timer_link).unwrap();
        assert!(!service_meta.is_symlink());
        assert!(!timer_meta.is_symlink());
        assert_eq!(
            std::fs::read_to_string(&service_link).unwrap(),
            "[Service]\nExecStart=/bin/true\n"
        );
        assert_eq!(
            std::fs::read_to_string(&timer_link).unwrap(),
            "[Timer]\nOnCalendar=daily\n"
        );
    }
}
