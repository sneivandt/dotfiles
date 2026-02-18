use anyhow::Result;

use super::{Context, Task, TaskResult};

/// Apply file permissions from chmod.ini.
pub struct ApplyFilePermissions;

impl Task for ApplyFilePermissions {
    fn name(&self) -> &'static str {
        "Apply file permissions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !ctx.config.chmod.is_empty()
    }

    fn dependencies(&self) -> Vec<&str> {
        vec!["Install symlinks"]
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        #[cfg(unix)]
        {
            use super::TaskStats;
            use std::os::unix::fs::PermissionsExt;

            let mut stats = TaskStats::new();
            for entry in &ctx.config.chmod {
                let target = ctx.home.join(format!(".{}", entry.path));

                if !target.exists() {
                    ctx.log
                        .debug(&format!("target missing, skipping: {}", target.display()));
                    stats.skipped += 1;
                    continue;
                }

                let mode = u32::from_str_radix(&entry.mode, 8)?;
                let current = std::fs::metadata(&target)?.permissions().mode() & 0o7777;

                if current == mode {
                    ctx.log.debug(&format!(
                        "ok: {} (already {})",
                        target.display(),
                        entry.mode
                    ));
                    stats.already_ok += 1;
                    continue;
                }

                if ctx.dry_run {
                    ctx.log.dry_run(&format!(
                        "would chmod {} {} (currently {:o})",
                        entry.mode,
                        target.display(),
                        current
                    ));
                    stats.changed += 1;
                    continue;
                }

                if target.is_dir() {
                    apply_recursive(&target, mode)?;
                } else {
                    let perms = std::fs::Permissions::from_mode(mode);
                    std::fs::set_permissions(&target, perms)?;
                }

                ctx.log
                    .debug(&format!("chmod {} {}", entry.mode, target.display()));
                stats.changed += 1;
            }

            Ok(stats.finish(ctx))
        }

        #[cfg(not(unix))]
        {
            let _ctx = ctx; // Suppress unused parameter warning
            Ok(TaskResult::Skipped(
                "chmod not supported on this platform".to_string(),
            ))
        }
    }
}

#[cfg(unix)]
fn apply_recursive(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                apply_recursive(&entry_path, mode)?;
            } else {
                std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }

    Ok(())
}
