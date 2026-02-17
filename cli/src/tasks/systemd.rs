use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Enable and start systemd user units.
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !ctx.config.units.is_empty() && exec::which("systemctl")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut changed = 0u32;
        let mut already_ok = 0u32;

        for unit in &ctx.config.units {
            let result = exec::run_unchecked("systemctl", &["--user", "is-enabled", &unit.name])?;
            if result.success {
                ctx.log
                    .debug(&format!("ok: {} (already enabled)", unit.name));
                already_ok += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log.dry_run(&format!("would enable: {}", unit.name));
                changed += 1;
                continue;
            }

            // Reload daemon before first enable
            if changed == 0
                && let Err(e) = exec::run("systemctl", &["--user", "daemon-reload"])
            {
                ctx.log.debug(&format!("daemon-reload failed: {e}"));
            }

            let result =
                exec::run_unchecked("systemctl", &["--user", "enable", "--now", &unit.name])?;

            if result.success {
                changed += 1;
                ctx.log.debug(&format!("enabled: {}", unit.name));
            } else {
                ctx.log.warn(&format!(
                    "failed to enable {}: {}",
                    unit.name,
                    result.stderr.trim()
                ));
            }
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("{changed} would change, {already_ok} already ok"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .info(&format!("{changed} changed, {already_ok} already ok"));
        Ok(TaskResult::Ok)
    }
}
