//! Task: apply Windows registry entries.

use anyhow::Result;

use crate::domains::system::config::registry::{RegistryEntry, validate};
use crate::domains::system::resources::registry::{RegistryResource, batch_check_states};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, run_batch_resource_task, task_metadata,
};
use crate::infra::config::Severity;

crate::engine::config_task! {
    /// Apply Windows registry settings.
    pub(crate) struct ApplyRegistry(Vec<RegistryEntry>);
}

const NAME: &str = "User registry settings";

impl Task for ApplyRegistry {
    task_metadata! {
        name: NAME,
        selector: "registry",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().has_registry()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let entries = self.config.get();
        if let Some(diagnostic) = validate(entries)
            .into_iter()
            .find(|item| item.severity == Severity::Error)
        {
            anyhow::bail!(
                "{} [{}]: {}",
                diagnostic.source,
                diagnostic.item,
                diagnostic.message
            );
        }
        let resources = entries.iter().map(RegistryResource::from_entry).collect();
        run_batch_resource_task(
            ctx,
            resources,
            |resources, _ctx| Ok(batch_check_states(resources)),
            |r, cached| {
                Ok(cached.get(&r.cache_key()).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "registry inventory omitted {}\\{}",
                        r.key_path,
                        r.value_name
                    )
                })?)
            },
            &ProcessOpts::lenient("configure"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::system::config::registry::RegistryEntry;
    use crate::engine::Task;
    use crate::engine::TaskResult;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{make_linux_context, make_windows_context};
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
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        assert!(!ApplyRegistry::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_when_guard_passes() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        assert!(ApplyRegistry::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_with_registry_entries() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        let task = ApplyRegistry::new(ConfigHandle::new(vec![entry()]));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ApplyRegistry::run
    // ------------------------------------------------------------------

    #[test]
    fn run_with_empty_registry_returns_not_applicable() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        let result = ApplyRegistry::new(ConfigHandle::new(vec![]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[cfg(not(windows))]
    #[test]
    fn invalid_dword_rejects_registry_task_before_inventory() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        let invalid = RegistryEntry {
            value_name: "Overflow".to_owned(),
            value_data: (u64::from(u32::MAX) + 1).to_string(),
            ..entry()
        };
        let error = ApplyRegistry::new(ConfigHandle::new(vec![entry(), invalid]))
            .run(&ctx)
            .unwrap_err();
        assert!(error.to_string().contains("[Overflow]: DWORD must fit"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unavailable_registry_is_counted_as_failed_not_missing() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        let result = ApplyRegistry::new(ConfigHandle::new(vec![entry()]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.failed_count() == 1
        ));
    }
}
