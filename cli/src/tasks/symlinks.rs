use anyhow::Result;
use std::path::Path;

use super::{Context, Task, TaskResult};

/// Create symlinks from symlinks/ to $HOME.
pub struct InstallSymlinks;

impl Task for InstallSymlinks {
    fn name(&self) -> &str {
        "Install symlinks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut count = 0u32;
        let mut skipped = 0u32;
        let mut already_ok = 0u32;

        for symlink in &ctx.config.symlinks {
            let source = ctx.symlinks_dir().join(&symlink.source);
            let target = compute_target(&ctx.home, &symlink.source);

            if !source.exists() {
                ctx.log
                    .debug(&format!("source missing, skipping: {}", symlink.source));
                skipped += 1;
                continue;
            }

            // Check if symlink already points to the correct source
            let is_correct = std::fs::read_link(&target)
                .map(|existing| paths_equal(&existing, &source))
                .unwrap_or(false);

            if ctx.dry_run {
                if is_correct {
                    ctx.log
                        .debug(&format!("ok: {} (already linked)", target.display()));
                    already_ok += 1;
                } else if target.is_dir()
                    && target
                        .symlink_metadata()
                        .map(|m| !m.is_symlink())
                        .unwrap_or(false)
                {
                    ctx.log.debug(&format!(
                        "target is a directory, would skip: {}",
                        target.display()
                    ));
                    skipped += 1;
                } else {
                    ctx.log.dry_run(&format!(
                        "would link {} -> {}",
                        target.display(),
                        source.display()
                    ));
                    count += 1;
                }
                continue;
            }

            // Ensure parent directory exists
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Skip if already correct
            if is_correct {
                already_ok += 1;
                continue;
            }

            // Remove existing target if it's a symlink or file
            if target.exists() || target.symlink_metadata().is_ok() {
                if target.is_dir() && !target.symlink_metadata()?.is_symlink() {
                    ctx.log.debug(&format!(
                        "target is a directory, skipping: {}",
                        target.display()
                    ));
                    skipped += 1;
                    continue;
                }
                remove_symlink(&target)?;
            }

            create_symlink(&source, &target)?;

            ctx.log.debug(&format!(
                "linked {} -> {}",
                target.display(),
                source.display()
            ));
            count += 1;
        }

        if ctx.dry_run {
            ctx.log.info(&format!(
                "{count} would change, {already_ok} already ok, {skipped} skipped"
            ));
            return Ok(TaskResult::DryRun);
        }

        ctx.log.info(&format!(
            "{count} created, {already_ok} already ok, {skipped} skipped"
        ));
        Ok(TaskResult::Ok)
    }
}

/// Remove installed symlinks.
pub struct UninstallSymlinks;

impl Task for UninstallSymlinks {
    fn name(&self) -> &str {
        "Remove symlinks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut count = 0u32;

        for symlink in &ctx.config.symlinks {
            let target = compute_target(&ctx.home, &symlink.source);
            let source = ctx.symlinks_dir().join(&symlink.source);

            // Only remove if it's a symlink pointing to our source
            if let Ok(link_target) = std::fs::read_link(&target)
                && link_target == source
            {
                if ctx.dry_run {
                    ctx.log
                        .dry_run(&format!("remove symlink: {}", target.display()));
                    count += 1;
                    continue;
                }
                remove_symlink(&target)?;
                ctx.log.debug(&format!("removed: {}", target.display()));
                count += 1;
            }
        }

        if ctx.dry_run {
            ctx.log.info(&format!("{count} would remove"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log.info(&format!("{count} symlinks removed"));
        Ok(TaskResult::Ok)
    }
}

/// Compare two paths, normalising the `\\?\` prefix that Windows
/// `read_link` prepends to extended-length paths.
fn paths_equal(a: &Path, b: &Path) -> bool {
    strip_win_prefix(a) == strip_win_prefix(b)
}

fn strip_win_prefix(p: &Path) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

/// Compute the target path in $HOME for a symlink source.
///
/// Symlink sources like "bashrc" map to "$HOME/.bashrc".
/// Sources like "config/git/config" map to "$HOME/.config/git/config".
/// Sources under "Documents/" or "AppData/" map to "$HOME/..." (no dot prefix).
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
/// We check the symlink metadata to determine the correct removal function.
fn remove_symlink(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        // Directory or directory symlink — remove_dir handles both on Windows
        std::fs::remove_dir(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Create a symlink (platform-specific).
fn create_symlink(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)?;
    }

    #[cfg(windows)]
    {
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(source, target)?;
        } else {
            std::os::windows::fs::symlink_file(source, target)?;
        }
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

    #[test]
    fn paths_equal_plain() {
        let a = PathBuf::from("C:\\Code\\dotfiles\\symlinks\\bashrc");
        let b = PathBuf::from("C:\\Code\\dotfiles\\symlinks\\bashrc");
        assert!(paths_equal(&a, &b));
    }

    #[test]
    fn paths_equal_with_unc_prefix() {
        let a = PathBuf::from(r"\\?\C:\Code\dotfiles\symlinks\bashrc");
        let b = PathBuf::from(r"C:\Code\dotfiles\symlinks\bashrc");
        assert!(paths_equal(&a, &b));
    }

    #[test]
    fn paths_equal_both_unc() {
        let a = PathBuf::from(r"\\?\C:\Code\dotfiles\symlinks\bashrc");
        let b = PathBuf::from(r"\\?\C:\Code\dotfiles\symlinks\bashrc");
        assert!(paths_equal(&a, &b));
    }

    #[test]
    fn paths_not_equal_different() {
        let a = PathBuf::from(r"\\?\C:\Code\dotfiles\symlinks\bashrc");
        let b = PathBuf::from(r"C:\Code\dotfiles\symlinks\zshrc");
        assert!(!paths_equal(&a, &b));
    }
}
