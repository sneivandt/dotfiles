use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Enable and start systemd user units.
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &str {
        "Configure systemd units"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !ctx.config.units.is_empty() && exec::which("systemctl")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            let mut would_enable = 0u32;
            let mut already = 0u32;
            for unit in &ctx.config.units {
                let result =
                    exec::run_unchecked("systemctl", &["--user", "is-enabled", &unit.name])?;
                if result.success {
                    ctx.log
                        .debug(&format!("ok: {} (already enabled)", unit.name));
                    already += 1;
                } else {
                    ctx.log.dry_run(&format!("would enable: {}", unit.name));
                    would_enable += 1;
                }
            }
            ctx.log.info(&format!(
                "{would_enable} would change, {already} already ok"
            ));
            return Ok(TaskResult::DryRun);
        }

        // Reload daemon first
        let _ = exec::run("systemctl", &["--user", "daemon-reload"]);

        let mut count = 0u32;
        for unit in &ctx.config.units {
            let result =
                exec::run_unchecked("systemctl", &["--user", "enable", "--now", &unit.name])?;

            if result.success {
                count += 1;
                ctx.log.debug(&format!("enabled: {}", unit.name));
            } else {
                ctx.log.warn(&format!(
                    "failed to enable {}: {}",
                    unit.name,
                    result.stderr.trim()
                ));
            }
        }

        ctx.log.info(&format!("{count} units enabled"));
        Ok(TaskResult::Ok)
    }
}
