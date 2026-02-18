use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::registry::RegistryResource;
use crate::resources::{Resource, ResourceState};

/// Apply Windows registry settings.
pub struct ApplyRegistry;

impl Task for ApplyRegistry {
    fn name(&self) -> &'static str {
        "Apply registry settings"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.has_registry() && !ctx.config.registry.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut stats = TaskStats::new();

        for entry in &ctx.config.registry {
            let resource = RegistryResource::from_entry(entry);

            // Check current state
            let resource_state = resource.current_state()?;
            match resource_state {
                ResourceState::Correct => {
                    ctx.log
                        .debug(&format!("ok: {} (already set)", resource.description()));
                    stats.already_ok += 1;
                    continue;
                }
                ResourceState::Incorrect { current } => {
                    ctx.log.debug(&format!(
                        "change needed: {} (currently {current})",
                        resource.description(),
                    ));
                    if ctx.dry_run {
                        ctx.log
                            .dry_run(&format!("would set registry: {}", resource.description()));
                        stats.changed += 1;
                        continue;
                    }
                }
                ResourceState::Missing => {
                    if ctx.dry_run {
                        ctx.log
                            .dry_run(&format!("would set registry: {}", resource.description()));
                        stats.changed += 1;
                        continue;
                    }
                }
                ResourceState::Invalid { reason } => {
                    ctx.log.debug(&format!("skipping: {reason}"));
                    stats.skipped += 1;
                    continue;
                }
            }

            // Apply the change
            match resource.apply() {
                Ok(_) => {
                    ctx.log
                        .debug(&format!("set registry: {}", resource.description()));
                    stats.changed += 1;
                }
                Err(e) => {
                    ctx.log.warn(&format!(
                        "failed to set registry: {} - {e}",
                        resource.description(),
                    ));
                    stats.skipped += 1;
                }
            }
        }

        Ok(stats.finish(ctx))
    }
}
