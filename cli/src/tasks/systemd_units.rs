use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::systemd_unit::SystemdUnitResource;

/// Enable and start systemd user units.
#[derive(Debug)]
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_systemd()
            && !ctx.config.units.is_empty()
            && ctx.executor.which("systemctl")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Reload systemd daemon once before processing (idempotent and fast)
        if !ctx.dry_run
            && let Err(e) = ctx.executor.run("systemctl", &["--user", "daemon-reload"])
        {
            ctx.log.debug(&format!("daemon-reload failed: {e}"));
        }

        let resources = ctx.config.units.iter().map(SystemdUnitResource::from_entry);
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "enable",
                fix_incorrect: false,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}
