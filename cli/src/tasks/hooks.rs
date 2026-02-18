use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};

/// Install git hooks from hooks/ into .git/hooks/.
pub struct GitHooks;

impl Task for GitHooks {
    fn name(&self) -> &'static str {
        "Install git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let hooks_src = ctx.hooks_dir();
        let hooks_dst = ctx.root().join(".git/hooks");

        if !hooks_dst.exists() {
            ctx.log.debug("creating .git/hooks directory");
            std::fs::create_dir_all(&hooks_dst)?;
        }

        let mut stats = TaskStats::new();
        for entry in std::fs::read_dir(&hooks_src)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let dst = hooks_dst.join(&filename);

            if ctx.dry_run {
                if dst.exists() {
                    let src_content = std::fs::read(&path)?;
                    let dst_content = std::fs::read(&dst)?;
                    if src_content == dst_content {
                        ctx.log.debug(&format!(
                            "ok: {} (already installed)",
                            filename.to_string_lossy()
                        ));
                        stats.already_ok += 1;
                    } else {
                        ctx.log.dry_run(&format!(
                            "would update hook: {}",
                            filename.to_string_lossy()
                        ));
                        stats.changed += 1;
                    }
                } else {
                    ctx.log.dry_run(&format!(
                        "would install hook: {}",
                        filename.to_string_lossy()
                    ));
                    stats.changed += 1;
                }
                continue;
            }

            // Remove stale/broken symlinks before comparing or copying
            if !dst.exists() && dst.symlink_metadata().is_ok() {
                ctx.log.debug(&format!(
                    "removing broken symlink: {}",
                    filename.to_string_lossy()
                ));
                std::fs::remove_file(&dst)?;
            }

            // Skip if already up to date
            if dst.exists() {
                let src_content = std::fs::read(&path)?;
                let dst_content = std::fs::read(&dst)?;
                if src_content == dst_content {
                    ctx.log.debug(&format!(
                        "ok: {} (already installed)",
                        filename.to_string_lossy()
                    ));
                    stats.already_ok += 1;
                    continue;
                }
                ctx.log.debug(&format!(
                    "updating hook: {} (content differs)",
                    filename.to_string_lossy()
                ));
                // Remove first to avoid Windows file-locking errors (os error 32)
                std::fs::remove_file(&dst)?;
            }

            std::fs::copy(&path, &dst)?;

            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&dst)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&dst, perms)?;
            }

            ctx.log
                .debug(&format!("installed hook: {}", filename.to_string_lossy()));
            stats.changed += 1;
        }

        Ok(stats.finish(ctx))
    }
}

/// Remove git hooks that were installed from hooks/.
pub struct UninstallHooks;

impl Task for UninstallHooks {
    fn name(&self) -> &'static str {
        "Remove git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git/hooks").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let hooks_src = ctx.hooks_dir();
        let hooks_dst = ctx.root().join(".git/hooks");

        let mut stats = TaskStats::new();
        for entry in std::fs::read_dir(&hooks_src)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let dst = hooks_dst.join(&filename);

            if !dst.exists() {
                ctx.log.debug(&format!(
                    "skip: {} (not installed)",
                    filename.to_string_lossy()
                ));
                stats.skipped += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log
                    .dry_run(&format!("remove hook: {}", filename.to_string_lossy()));
                stats.changed += 1;
                continue;
            }

            std::fs::remove_file(&dst)?;
            ctx.log
                .debug(&format!("removed hook: {}", filename.to_string_lossy()));
            stats.changed += 1;
        }

        Ok(stats.finish(ctx))
    }
}
