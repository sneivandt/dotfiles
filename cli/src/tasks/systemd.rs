use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;

/// Enable and start systemd user units.
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_systemd() && !ctx.config.units.is_empty() && exec::which("systemctl")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut stats = TaskStats::new();

        for unit in &ctx.config.units {
            let result = exec::run_unchecked("systemctl", &["--user", "is-enabled", &unit.name])?;
            if result.success {
                ctx.log
                    .debug(&format!("ok: {} (already enabled)", unit.name));
                stats.already_ok += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log.dry_run(&format!("would enable: {}", unit.name));
                stats.changed += 1;
                continue;
            }

            // Reload daemon before first enable
            if stats.changed == 0
                && let Err(e) = exec::run("systemctl", &["--user", "daemon-reload"])
            {
                ctx.log.debug(&format!("daemon-reload failed: {e}"));
            }

            let result =
                exec::run_unchecked("systemctl", &["--user", "enable", "--now", &unit.name])?;

            if result.success {
                stats.changed += 1;
                ctx.log.debug(&format!("enabled: {}", unit.name));
            } else {
                ctx.log.warn(&format!(
                    "failed to enable {}: {}",
                    unit.name,
                    result.stderr.trim()
                ));
            }
        }

        Ok(stats.finish(ctx))
    }
}
