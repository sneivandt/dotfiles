//! Tasks: install and uninstall symlinks.
use anyhow::Result;
use std::path::Path;

use super::{
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove, task_deps,
};
use crate::resources::symlink::SymlinkResource;

/// Build [`SymlinkResource`] instances from the loaded config.
fn build_resources(ctx: &Context) -> Vec<SymlinkResource> {
    let symlinks = ctx.config_read().symlinks.clone();
    let symlinks_dir = ctx.symlinks_dir();
    symlinks
        .iter()
        .map(|s| {
            let target = s.target.as_ref().map_or_else(
                || compute_target(&ctx.home, &s.source),
                |explicit| ctx.home.join(explicit),
            );
            SymlinkResource::new(symlinks_dir.join(&s.source), target)
        })
        .collect()
}

/// Create symlinks from symlinks/ to $HOME.
#[derive(Debug)]
pub struct InstallSymlinks;

impl Task for InstallSymlinks {
    fn name(&self) -> &'static str {
        "Install symlinks"
    }

    task_deps![
        super::reload_config::ReloadConfig,
        super::developer_mode::EnableDeveloperMode
    ];

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(ctx, build_resources(ctx), &ProcessOpts::apply_all("link"))
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

/// Compute the target path in $HOME for a symlink source.
///
/// Symlink sources like "bashrc" map to "$HOME/.bashrc".
/// Sources like "config/git/config" map to "$HOME/.config/git/config".
/// Sources under "Documents/" or "`AppData`/" map to "$HOME/..." (no dot prefix).
fn compute_target(home: &Path, source: &str) -> std::path::PathBuf {
    let lower = source.to_ascii_lowercase();
    if lower.starts_with("documents/") || lower.starts_with("appdata/") {
        home.join(source)
    } else {
        home.join(format!(".{source}"))
    }
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
    fn target_for_documents() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "Documents/PowerShell/profile.ps1");
        assert_eq!(
            target,
            PathBuf::from("/home/user/Documents/PowerShell/profile.ps1")
        );
    }

    #[test]
    fn target_for_appdata() {
        let home = PathBuf::from("C:/Users/user");
        let target = compute_target(&home, "AppData/Roaming/Code/User/settings.json");
        assert_eq!(
            target,
            PathBuf::from("C:/Users/user/AppData/Roaming/Code/User/settings.json")
        );
    }

    #[test]
    fn target_for_appdata_lowercase() {
        let home = PathBuf::from("C:/Users/user");
        let target = compute_target(&home, "appdata/Local/something");
        assert_eq!(
            target,
            PathBuf::from("C:/Users/user/appdata/Local/something")
        );
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
    fn install_should_run_false_when_no_symlinks_configured() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!InstallSymlinks.should_run(&ctx));
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
        let mut ctx = make_linux_context(config);
        ctx.home = home_dir.path().to_path_buf();

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
        let mut ctx = make_linux_context(config);
        ctx.home = home_dir.path().to_path_buf();

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
}
