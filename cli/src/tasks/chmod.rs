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

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut count = 0u32;
            let mut already_ok = 0u32;
            for entry in &ctx.config.chmod {
                let target = ctx.home.join(format!(".{}", entry.path));

                if !target.exists() {
                    ctx.log
                        .debug(&format!("target missing, skipping: {}", target.display()));
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
                    already_ok += 1;
                    continue;
                }

                if ctx.dry_run {
                    ctx.log.dry_run(&format!(
                        "would chmod {} {} (currently {:o})",
                        entry.mode,
                        target.display(),
                        current
                    ));
                    count += 1;
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
                count += 1;
            }

            if ctx.dry_run {
                ctx.log
                    .info(&format!("{count} would change, {already_ok} already ok"));
                return Ok(TaskResult::DryRun);
            }

            ctx.log
                .info(&format!("{count} changed, {already_ok} already ok"));
            Ok(TaskResult::Ok)
        }

        #[cfg(not(unix))]
        {
            let _ = ctx;
            Ok(TaskResult::Skipped(
                "chmod not supported on this platform".to_string(),
            ))
        }
    }
}

#[cfg(unix)]
fn apply_recursive(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms.clone())?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                apply_recursive(&entry_path, mode)?;
            } else {
                std::fs::set_permissions(&entry_path, perms.clone())?;
            }
        }
    }

    Ok(())
}
