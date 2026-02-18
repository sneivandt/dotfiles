use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::chmod::ChmodResource;
use crate::resources::{Resource, ResourceState};

/// Apply file permissions from chmod.ini.
pub struct ApplyFilePermissions;

impl Task for ApplyFilePermissions {
    fn name(&self) -> &'static str {
        "Apply file permissions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_chmod() && !ctx.config.chmod.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut stats = TaskStats::new();

        for entry in &ctx.config.chmod {
            let resource = ChmodResource::from_entry(entry, &ctx.home);

            // Check current state
            let resource_state = resource.current_state()?;
            match resource_state {
                ResourceState::Invalid { reason } => {
                    ctx.log.debug(&format!("skipping: {reason}"));
                    stats.skipped += 1;
                    continue;
                }
                ResourceState::Correct => {
                    ctx.log
                        .debug(&format!("ok: {} (already correct)", resource.description()));
                    stats.already_ok += 1;
                    continue;
                }
                ResourceState::Incorrect { current } => {
                    if ctx.dry_run {
                        ctx.log.dry_run(&format!(
                            "would chmod {} (currently {current})",
                            resource.description(),
                        ));
                        stats.changed += 1;
                        continue;
                    }
                }
                ResourceState::Missing => {
                    // This shouldn't happen for chmod, but handle it
                    stats.skipped += 1;
                    continue;
                }
            }

            // Apply the change
            resource.apply()?;
            ctx.log.debug(&format!("chmod {}", resource.description()));
            stats.changed += 1;
        }

        Ok(stats.finish(ctx))
    }
}
