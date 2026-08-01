//! Task: configure GitHub Copilot CLI settings.

use anyhow::Result;

use crate::domains::ai::config::copilot::CopilotSetting;
use crate::domains::ai::resources::copilot_settings::CopilotSettingResource;
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, configured_task_result, run_resource_task,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Configure Copilot CLI settings from copilot.toml.
///
/// Each managed key is converged inside `~/.copilot/settings.json` without
/// disturbing keys the Copilot CLI manages itself.  Processing is forced
/// sequential because every resource reads and rewrites the same file.
#[derive(Debug)]
pub struct ConfigureCopilot {
    config: ConfigHandle<Vec<CopilotSetting>>,
}

const NAME: &str = "Copilot settings";

impl ConfigureCopilot {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<CopilotSetting>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        let settings = self.config.read().to_vec();
        run_resource_task(
            ctx,
            announce,
            settings,
            |s, ctx| {
                CopilotSettingResource::new(
                    s.key.clone(),
                    s.json_value(),
                    ctx.paths().home().join(".copilot").join("settings.json"),
                )
            },
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

impl Task for ConfigureCopilot {
    task_metadata! {
        name: NAME,
        selector: "copilot",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)?))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::domains::ai::config::copilot::CopilotSetting;
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_is_true_without_explicit_guard() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigureCopilot::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn run_is_not_applicable_without_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let result = ConfigureCopilot::new(ConfigHandle::new(vec![]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_with_settings_converges() {
        let dir = tempfile::tempdir().unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config).with_home(dir.path().to_path_buf());
        let task = ConfigureCopilot::new(ConfigHandle::new(vec![CopilotSetting {
            key: "model".to_string(),
            value: toml::Value::String("claude-opus-4.8".to_string()),
        }]));
        let _result = task.run(&ctx).unwrap();

        let written =
            std::fs::read_to_string(dir.path().join(".copilot").join("settings.json")).unwrap();
        assert!(written.contains("claude-opus-4.8"));
    }
}
