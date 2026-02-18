use anyhow::Result;
use std::path::Path;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::symlink::SymlinkResource;
use crate::resources::{Resource, ResourceState};

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
        let mut stats = TaskStats::new();

        for symlink in &ctx.config.symlinks {
            let source = ctx.symlinks_dir().join(&symlink.source);
            let target = compute_target(&ctx.home, &symlink.source);

            let resource = SymlinkResource::new(source, target.clone());

            // Check current state
            let resource_state = resource.current_state()?;
            match resource_state {
                ResourceState::Invalid { reason } => {
                    ctx.log
                        .debug(&format!("skipping {}: {reason}", symlink.source));
                    stats.skipped += 1;
                    continue;
                }
                ResourceState::Correct => {
                    ctx.log
                        .debug(&format!("ok: {} (already linked)", target.display()));
                    stats.already_ok += 1;
                    continue;
                }
                ResourceState::Incorrect { current } => {
                    if ctx.dry_run {
                        ctx.log.dry_run(&format!(
                            "would link {} -> {} (currently {current})",
                            target.display(),
                            resource.description().split(" -> ").nth(1).unwrap_or(""),
                        ));
                        stats.changed += 1;
                        continue;
                    }
                }
                ResourceState::Missing => {
                    if ctx.dry_run {
                        ctx.log
                            .dry_run(&format!("would link {}", resource.description()));
                        stats.changed += 1;
                        continue;
                    }
                }
            }

            // Apply the change
            resource.apply()?;
            ctx.log.debug(&format!("linked {}", resource.description()));
            stats.changed += 1;
        }

        Ok(stats.finish(ctx))
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
        let mut stats = TaskStats::new();

        for symlink in &ctx.config.symlinks {
            let target = compute_target(&ctx.home, &symlink.source);
            let source = ctx.symlinks_dir().join(&symlink.source);

            let resource = SymlinkResource::new(source, target.clone());

            // Only remove if it's a symlink pointing to our source
            if resource.current_state()? == ResourceState::Correct {
                // It's correctly pointing to our source, remove it
                if ctx.dry_run {
                    ctx.log
                        .dry_run(&format!("would remove symlink: {}", target.display()));
                    stats.changed += 1;
                    continue;
                }

                // Use the resource's internal remove logic via SymlinkResource
                // For now, we need to manually remove since Resource trait doesn't have uninstall
                remove_symlink(&target)?;
                ctx.log.debug(&format!("removed: {}", target.display()));
                stats.changed += 1;
            }
            // Not our symlink or doesn't exist, skip
        }

        Ok(stats.finish(ctx))
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

/// Remove a symlink, handling platform differences.
///
/// On Windows, directory symlinks must be removed with `remove_dir` (not `remove_file`).
/// Rust's `symlink_metadata().is_dir()` returns `false` for symlinks, so we check
/// the raw `FILE_ATTRIBUTE_DIRECTORY` flag to detect directory symlinks.
/// If `remove_dir` still fails with OS error 5 (access denied), we fall back
/// to `cmd /c rmdir` which runs in a separate process.
fn remove_symlink(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if is_dir_like(&meta) {
        match std::fs::remove_dir(path) {
            Ok(()) => {}
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(5) => {
                remove_dir_fallback(path)?;
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Check if metadata represents a directory-like entry.
/// On Windows, `symlink_metadata().is_dir()` returns `false` for directory symlinks,
/// so we check the raw `FILE_ATTRIBUTE_DIRECTORY` bit instead.
fn is_dir_like(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x10 != 0 // FILE_ATTRIBUTE_DIRECTORY
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Fallback directory removal on Windows using `cmd /c rmdir`.
/// This spawns a separate process that doesn't hold any handles from the
/// current process, which can resolve "Access is denied" errors.
#[cfg(windows)]
fn remove_dir_fallback(path: &Path) -> Result<()> {
    use anyhow::Context as _;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("cmd")
        .arg("/c")
        .arg("rmdir")
        .arg("/q")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("failed to run rmdir")?;
    if !output.status.success() {
        anyhow::bail!(
            "remove directory/symlink '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
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
