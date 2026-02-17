use anyhow::Result;

use super::{Context, Task, TaskResult};

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
            std::fs::create_dir_all(&hooks_dst)?;
        }

        let mut count = 0u32;
        let mut skipped = 0u32;
        for entry in std::fs::read_dir(&hooks_src)? {
            let entry = entry?;
            let path = entry.path();

            // Skip non-files
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
                        skipped += 1;
                    } else {
                        ctx.log.dry_run(&format!(
                            "would update hook: {}",
                            filename.to_string_lossy()
                        ));
                        count += 1;
                    }
                } else {
                    ctx.log.dry_run(&format!(
                        "would install hook: {}",
                        filename.to_string_lossy()
                    ));
                    count += 1;
                }
                continue;
            }

            // Skip if already up to date
            if dst.exists() {
                let src_content = std::fs::read(&path)?;
                let dst_content = std::fs::read(&dst)?;
                if src_content == dst_content {
                    skipped += 1;
                    continue;
                }
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
            count += 1;
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("{count} would change, {skipped} already ok"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .info(&format!("{count} changed, {skipped} already ok"));
        Ok(TaskResult::Ok)
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

        let mut count = 0u32;
        let mut skipped = 0u32;
        for entry in std::fs::read_dir(&hooks_src)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let dst = hooks_dst.join(&filename);

            if !dst.exists() {
                skipped += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log
                    .dry_run(&format!("remove hook: {}", filename.to_string_lossy()));
                count += 1;
                continue;
            }

            std::fs::remove_file(&dst)?;
            ctx.log
                .debug(&format!("removed hook: {}", filename.to_string_lossy()));
            count += 1;
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("{count} would change, {skipped} already gone"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log.info(&format!("{count} hooks removed"));
        Ok(TaskResult::Ok)
    }
}
