use anyhow::Result;
use std::any::TypeId;

use super::{Context, ProcessOpts, Task, TaskResult, process_resource_states};
use crate::resources::registry::{RegistryResource, batch_check_values};

/// Apply Windows registry settings.
#[derive(Debug)]
pub struct ApplyRegistry;

impl Task for ApplyRegistry {
    fn name(&self) -> &'static str {
        "Apply registry settings"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::reload_config::ReloadConfig>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.has_registry() && !ctx.config_read().registry.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let registry_entries: Vec<_> = ctx.config_read().registry.clone();
        let resources: Vec<RegistryResource> = registry_entries
            .iter()
            .map(RegistryResource::from_entry)
            .collect();

        ctx.log.debug(&format!(
            "batch-checking {} registry values",
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::registry::RegistryEntry;
    use crate::exec::Executor;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, empty_config, make_context};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(!ApplyRegistry.should_run(&ctx));
    }

    #[test]
    fn should_run_false_on_windows_when_registry_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Arc::new(Platform::new(Os::Windows, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(!ApplyRegistry.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_with_registry_entries() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.registry.push(RegistryEntry {
            key_path: r"HKCU:\Console".to_string(),
            value_name: "QuickEdit".to_string(),
            value_data: "1".to_string(),
        });
        let platform = Arc::new(Platform::new(Os::Windows, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(ApplyRegistry.should_run(&ctx));
    }
}
