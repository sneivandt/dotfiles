use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::resources::registry::{RegistryResource, batch_check_values};
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

        // Build all resources up front
        let resources: Vec<RegistryResource> = ctx
            .config
            .registry
            .iter()
            .map(RegistryResource::from_entry)
            .collect();

        ctx.log.debug(&format!(
            "batch-checking {} registry values in a single PowerShell call",
            resources.len()
        ));

        // Single PowerShell invocation to check every value at once
        let cached = batch_check_values(&resources)?;

        for resource in &resources {
            let cache_key = format!("{}\\{}", resource.key_path, resource.value_name);
            let current_value = cached.get(&cache_key).and_then(|v| v.as_deref());
            let resource_state = resource.state_from_cached(current_value);

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
