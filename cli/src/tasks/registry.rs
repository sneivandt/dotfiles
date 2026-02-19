use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resource_states};
use crate::resources::registry::{RegistryResource, batch_check_values};

/// Apply Windows registry settings.
#[derive(Debug)]
pub struct ApplyRegistry;

impl Task for ApplyRegistry {
    fn name(&self) -> &'static str {
        "Apply registry settings"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.has_registry() && !ctx.config.registry.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
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

        let cached = batch_check_values(&resources)?;

        let resource_states = resources.into_iter().map(|r| {
            let key = format!("{}\\{}", r.key_path, r.value_name);
            let val = cached.get(&key).and_then(|v| v.as_deref());
            let state = r.state_from_cached(val);
            (r, state)
        });

        process_resource_states(
            ctx,
            resource_states,
            &ProcessOpts {
                verb: "set registry",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}
