//! Task: configure supported agent harness settings.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting};
use crate::domains::ai::resources::agent_settings::{AgentSettingResource, SettingsFormat};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, configured_task_result, run_resource_task,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Configure agent harness settings from `agent-settings.toml`.
///
/// Each managed key is converged inside its harness's user settings document
/// without disturbing unmanaged keys. Processing is forced sequential because
/// multiple resources can read and rewrite the same file.
#[derive(Debug)]
pub struct ConfigureAgentSettings {
    config: ConfigHandle<Vec<AgentSetting>>,
}

const NAME: &str = "Agent settings";

impl ConfigureAgentSettings {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<AgentSetting>>) -> Self {
        Self { config }
    }

    fn resource(setting: AgentSetting, home: &Path) -> AgentSettingResource {
        let (format, path) = target_document(setting.target, home);
        AgentSettingResource::new(
            setting.target.name().to_string(),
            setting.key,
            setting.value,
            format,
            path,
        )
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        let settings = self.config.read().to_vec();
        let home = ctx.paths().home().to_path_buf();
        run_resource_task(
            ctx,
            announce,
            settings,
            move |setting, _ctx| Self::resource(setting, &home),
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

fn target_document(target: AgentHarness, home: &Path) -> (SettingsFormat, PathBuf) {
    match target {
        AgentHarness::Copilot => (
            SettingsFormat::Json,
            home.join(".copilot").join("settings.json"),
        ),
        AgentHarness::Codex => (
            SettingsFormat::Toml,
            home.join(".codex").join("config.toml"),
        ),
    }
}

impl Task for ConfigureAgentSettings {
    task_metadata! {
        name: NAME,
        selector: "agent-settings",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting};
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn run_is_not_applicable_without_settings() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let result = ConfigureAgentSettings::new(ConfigHandle::new(vec![]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_with_settings_converges() {
        let dir = tempfile::tempdir().unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config).with_home(dir.path().to_path_buf());
        let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![
            AgentSetting {
                target: AgentHarness::Copilot,
                key: "model".to_string(),
                value: toml::Value::String("claude-opus-4.8".to_string()),
            },
            AgentSetting {
                target: AgentHarness::Codex,
                key: "model_reasoning_effort".to_string(),
                value: toml::Value::String("high".to_string()),
            },
        ]));
        let _result = task.run(&ctx).unwrap();

        let copilot_settings =
            std::fs::read_to_string(dir.path().join(".copilot").join("settings.json")).unwrap();
        assert!(copilot_settings.contains("claude-opus-4.8"));

        let codex_config =
            std::fs::read_to_string(dir.path().join(".codex").join("config.toml")).unwrap();
        assert!(codex_config.contains("model_reasoning_effort = \"high\""));
    }

    #[test]
    fn target_documents_use_shared_user_locations() {
        let home = Path::new("/home/test");
        assert_eq!(
            target_document(AgentHarness::Copilot, home),
            (
                SettingsFormat::Json,
                home.join(".copilot").join("settings.json")
            )
        );
        assert_eq!(
            target_document(AgentHarness::Codex, home),
            (
                SettingsFormat::Toml,
                home.join(".codex").join("config.toml")
            )
        );
    }
}
