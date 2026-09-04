//! Task: apply Windows registry entries.

use anyhow::Result;

use crate::domains::system::config::registry::RegistryEntry;
use crate::domains::system::resources::registry::{RegistryResource, batch_check_values};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, run_batch_resource_task, task_metadata,
};
use crate::infra::ConfigHandle;

/// Apply Windows registry settings.
#[derive(Debug)]
pub struct ApplyRegistry {
    config: ConfigHandle<Vec<RegistryEntry>>,
}

const NAME: &str = "Windows registry";

impl ApplyRegistry {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<RegistryEntry>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<TaskResult> {
        let entries = self.config.read().to_vec();
        run_batch_resource_task(
            ctx,
            announce,
            entries,
            |entry, _ctx| RegistryResource::from_entry(&entry),
            |resources, _ctx| batch_check_values(resources),
            |r, cached| {
                let key = format!("{}\\{}", r.key_path, r.value_name);
                let val = cached.get(&key).and_then(Option::as_ref);
                Ok(r.state_from_cached(val))
            },
            &ProcessOpts::lenient("configure"),
        )
    }
}

impl Task for ApplyRegistry {
    task_metadata! {
        name: NAME,
        selector: "registry",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().has_registry()
    }

    fn run_configured(&self, ctx: &Context) -> Result<TaskResult> {
        self.process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.process(ctx, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::system::config::registry::RegistryEntry;
    use crate::engine::Task;
    use crate::engine::TaskResult;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    fn entry() -> RegistryEntry {
        RegistryEntry {
            key_path: r"HKCU:\Console".to_string(),
            value_name: "QuickEdit".to_string(),
            value_data: "1".to_string(),
            value_type: crate::domains::system::config::registry::RegistryValueType::Dword,
            origin: None,
        }
    }

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ApplyRegistry::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(ApplyRegistry::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_with_registry_entries() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        let task = ApplyRegistry::new(ConfigHandle::new(vec![entry()]));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ApplyRegistry::run
    // ------------------------------------------------------------------

    #[test]
    fn run_with_empty_registry_returns_not_applicable() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        let result = ApplyRegistry::new(ConfigHandle::new(vec![]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[cfg(not(windows))]
    #[test]
    fn run_with_entries_on_non_windows_skips_gracefully() {
        // On non-Windows, batch_check_values() returns an empty map.
        // Every entry therefore has state Missing, and apply() returns an
        // error ("registry operations are only supported on Windows").
        // Because ProcessOpts is lenient, each error is caught and counted
        // as a non-fatal failure rather than propagating the error.
        // Use a Windows-platform context so the task logic runs (should_run
        // would normally gate this, but run() is called directly in unit tests).
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        let result = ApplyRegistry::new(ConfigHandle::new(vec![entry()]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.failed_count() == 1
        ));
    }
}
