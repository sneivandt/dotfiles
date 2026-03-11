//! Task: apply Windows registry entries.

use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, run_batch_resource_task, task_deps};
use crate::resources::registry::{RegistryResource, batch_check_values};

/// Apply Windows registry settings.
#[derive(Debug)]
pub struct ApplyRegistry;

impl Task for ApplyRegistry {
    fn name(&self) -> &'static str {
        "Apply registry settings"
    }

    task_deps![super::reload_config::ReloadConfig];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.has_registry()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        if !ctx.platform.has_registry() {
            return Ok(None);
        }
        let items: Vec<_> = ctx.config_read().registry.clone();
        if items.is_empty() {
            return Ok(None);
        }
        run_batch_resource_task(
            ctx,
            items,
            |entries, _ctx| {
                let resources: Vec<RegistryResource> =
                    entries.iter().map(RegistryResource::from_entry).collect();
                batch_check_values(&resources)
            },
            |entry, _ctx| RegistryResource::from_entry(&entry),
            |r, cached| {
                let key = format!("{}\\{}", r.key_path, r.value_name);
                let val = cached.get(&key).and_then(|v| v.as_deref());
                r.state_from_cached(val)
            },
            &ProcessOpts::lenient("set registry"),
        )
        .map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items: Vec<_> = ctx.config_read().registry.clone();
        if items.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        run_batch_resource_task(
            ctx,
            items,
            |entries, _ctx| {
                let resources: Vec<RegistryResource> =
                    entries.iter().map(RegistryResource::from_entry).collect();
                batch_check_values(&resources)
            },
            |entry, _ctx| RegistryResource::from_entry(&entry),
            |r, cached| {
                let key = format!("{}\\{}", r.key_path, r.value_name);
                let val = cached.get(&key).and_then(|v| v.as_deref());
                r.state_from_cached(val)
            },
            &ProcessOpts::lenient("set registry"),
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::registry::RegistryEntry;
    use crate::tasks::Task;
    use crate::tasks::TaskResult;
    use crate::tasks::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ApplyRegistry.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(ApplyRegistry.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_with_registry_entries() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.registry.push(RegistryEntry {
            key_path: r"HKCU:\Console".to_string(),
            value_name: "QuickEdit".to_string(),
            value_data: "1".to_string(),
        });
        let ctx = make_windows_context(config);
        assert!(ApplyRegistry.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ApplyRegistry::run
    // ------------------------------------------------------------------

    #[test]
    fn run_with_empty_registry_returns_not_applicable() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        let result = ApplyRegistry.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_with_entries_on_non_windows_skips_gracefully() {
        // On non-Windows, batch_check_values() returns an empty map.
        // Every entry therefore has state Missing, and apply() returns an
        // error ("registry operations are only supported on Windows").
        // Because ProcessOpts uses no_bail(), each error is caught and counted
        // as skipped — the task still returns Ok rather than propagating the error.
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.registry.push(RegistryEntry {
            key_path: r"HKCU:\Console".to_string(),
            value_name: "QuickEdit".to_string(),
            value_data: "1".to_string(),
        });
        // Use a Windows-platform context so the task logic runs (should_run
        // would normally gate this, but run() is called directly in unit tests).
        let ctx = make_windows_context(config);
        let result = ApplyRegistry.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }
}
