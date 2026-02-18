use anyhow::Result;
use std::path::Path;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove};
use crate::resources::symlink::SymlinkResource;

/// Create symlinks from symlinks/ to $HOME.
pub struct InstallSymlinks;

impl Task for InstallSymlinks {
    fn name(&self) -> &'static str {
        "Install symlinks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = ctx.config.symlinks.iter().map(|s| {
            SymlinkResource::new(
                ctx.symlinks_dir().join(&s.source),
                compute_target(&ctx.home, &s.source),
            )
        });
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "link",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: true,
            },
        )
    }
}

/// Remove installed symlinks.
pub struct UninstallSymlinks;

impl Task for UninstallSymlinks {
    fn name(&self) -> &'static str {
        "Remove symlinks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = ctx.config.symlinks.iter().map(|s| {
            SymlinkResource::new(
                ctx.symlinks_dir().join(&s.source),
                compute_target(&ctx.home, &s.source),
            )
        });
        process_resources_remove(ctx, resources, "remove")
    }
}

/// Compute the target path in $HOME for a symlink source.
///
/// Symlink sources like "bashrc" map to "$HOME/.bashrc".
/// Sources like "config/git/config" map to "$HOME/.config/git/config".
/// Sources under "Documents/" or "`AppData`/" map to "$HOME/..." (no dot prefix).
fn compute_target(home: &Path, source: &str) -> std::path::PathBuf {
    if source.starts_with("Documents/")
        || source.starts_with("documents/")
        || source.starts_with("AppData/")
    {
        home.join(source)
    } else {
        home.join(format!(".{source}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn target_for_ssh() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "ssh/config");
        assert_eq!(target, PathBuf::from("/home/user/.ssh/config"));
    }
}
