use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;
use crate::resources::systemd_unit::SystemdUnitResource;
use crate::resources::{Resource, ResourceChange, ResourceState};

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
        let mut daemon_reloaded = false;

        for unit in &ctx.config.units {
            let resource = SystemdUnitResource::from_entry(unit);

            match resource.current_state()? {
                ResourceState::Correct => {
                    ctx.log
                        .debug(&format!("ok: {} (already enabled)", resource.description()));
                    stats.already_ok += 1;
                }
                ResourceState::Missing => {
                    if ctx.dry_run {
                        ctx.log
                            .dry_run(&format!("would enable: {}", resource.description()));
                        stats.changed += 1;
                        continue;
                    }

                    // Reload daemon before first enable
                    if !daemon_reloaded {
                        if let Err(e) = exec::run("systemctl", &["--user", "daemon-reload"]) {
                            ctx.log.debug(&format!("daemon-reload failed: {e}"));
                        }
                        daemon_reloaded = true;
                    }

                    match resource.apply() {
                        Ok(ResourceChange::Applied) => {
                            ctx.log
                                .debug(&format!("enabled: {}", resource.description()));
                            stats.changed += 1;
                        }
                        Ok(ResourceChange::Skipped { reason }) => {
                            ctx.log.warn(&format!(
                                "failed to enable {}: {reason}",
                                resource.description()
                            ));
                        }
                        Ok(ResourceChange::AlreadyCorrect) => {
                            stats.already_ok += 1;
                        }
                        Err(e) => {
                            ctx.log
                                .warn(&format!("failed to enable {}: {e}", resource.description()));
                        }
                    }
                }
                ResourceState::Incorrect { current } => {
                    ctx.log.debug(&format!(
                        "unit {} unexpected state: {current}",
                        resource.description()
                    ));
                    stats.skipped += 1;
                }
                ResourceState::Invalid { reason } => {
                    ctx.log
                        .debug(&format!("skipping {}: {reason}", resource.description()));
                    stats.skipped += 1;
                }
            }
        }

        Ok(stats.finish(ctx))
    }
}
