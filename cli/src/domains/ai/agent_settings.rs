//! Task: configure supported agent harness settings.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting, validate_conflicts};
use crate::domains::ai::resources::agent_settings::{AgentSettingResource, SettingsFormat};
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};
use crate::infra::ConfigHandle;

/// Configure agent harness settings from `agent-settings.toml`.
///
/// Each managed key is converged inside its harness's user settings document
/// without disturbing unmanaged keys. Processing is forced sequential because
/// multiple resources can read and rewrite the same file. Contradictory values
/// for the same harness/key are rejected before any document is changed.
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

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings = self.config.read().to_vec();
        if let Some(conflict) = validate_conflicts(&settings).first() {
            anyhow::bail!("{}: {}", conflict.item, conflict.message);
        }
        let resources = settings
            .into_iter()
            .map(|setting| Self::resource(setting, ctx.home()))
            .collect();
        run_resource_task(
            ctx,
            resources,
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting};
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context, task_batch};
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
        let ctx = make_linux_context(config)
            .with_home(dir.path().to_path_buf())
            .with_parallel(true);
        let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![
            AgentSetting {
                target: AgentHarness::Copilot,
                key: "model".to_string(),
                value: toml::Value::String("claude-opus-4.8".to_string()),
            },
            AgentSetting {
                target: AgentHarness::Copilot,
                key: "footer.showBranch".to_string(),
                value: toml::Value::Boolean(true),
            },
            AgentSetting {
                target: AgentHarness::Codex,
                key: "model_reasoning_effort".to_string(),
                value: toml::Value::String("high".to_string()),
            },
            AgentSetting {
                target: AgentHarness::Codex,
                key: "tui.theme".to_string(),
                value: toml::Value::String("fixture-theme".to_string()),
            },
        ]));
        let result = task.run(&ctx).unwrap();
        assert_eq!(task_batch(&result).changed_count(), 4);

        let copilot_settings =
            std::fs::read_to_string(dir.path().join(".copilot").join("settings.json")).unwrap();
        let copilot: serde_json::Value = serde_json::from_str(&copilot_settings).unwrap();
        assert_eq!(
            copilot,
            serde_json::json!({
                "model": "claude-opus-4.8", "footer": { "showBranch": true },
            })
        );

        let codex_config =
            std::fs::read_to_string(dir.path().join(".codex").join("config.toml")).unwrap();
        let codex: toml::Table = toml::from_str(&codex_config).unwrap();
        assert_eq!(codex["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(codex["tui"]["theme"].as_str(), Some("fixture-theme"));
        let repeated = task.run(&ctx).unwrap();
        assert_eq!(task_batch(&repeated).changed_count(), 0);
        assert_eq!(task_batch(&repeated).already_ok_count(), 4);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".copilot/settings.json")).unwrap(),
            copilot_settings
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap(),
            codex_config
        );
    }

    #[test]
    fn dry_run_leaves_missing_and_existing_agent_documents_untouched() {
        for existing in [false, true] {
            let home = tempfile::tempdir_in(".").unwrap();
            let json_path = home.path().join(".copilot/settings.json");
            let toml_path = home.path().join(".codex/config.toml");
            let json_before = r#"{"model":"old","unmanaged":true}"#;
            let toml_before = "model = \"old\"\nunmanaged = true\n";
            if existing {
                for (path, content) in [(&json_path, json_before), (&toml_path, toml_before)] {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, content).unwrap();
                }
            }
            let ctx = make_linux_context(empty_config(home.path().to_path_buf()))
                .with_home(home.path().to_path_buf())
                .with_dry_run(true);
            let settings = [AgentHarness::Copilot, AgentHarness::Codex]
                .into_iter()
                .map(|target| AgentSetting {
                    target,
                    key: "model".to_string(),
                    value: toml::Value::String("new".to_string()),
                })
                .collect();
            let result = ConfigureAgentSettings::new(ConfigHandle::new(settings))
                .run(&ctx)
                .unwrap();
            assert_eq!(task_batch(&result).changed_count(), 2);
            for (path, before) in [(&json_path, json_before), (&toml_path, toml_before)] {
                if existing {
                    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
                } else {
                    assert!(!path.parent().unwrap().exists());
                }
            }
        }
    }

    #[test]
    fn conflicting_settings_fail_before_changing_any_document() {
        for target in [AgentHarness::Copilot, AgentHarness::Codex] {
            for dry_run in [false, true] {
                for existing in [false, true] {
                    let home = tempfile::tempdir_in(".").unwrap();
                    let (_, path) = target_document(target, home.path());
                    let before = match target {
                        AgentHarness::Copilot => "{\"model\":\"original\",\"unmanaged\":true}\n",
                        AgentHarness::Codex => "model = \"original\"\nunmanaged = true\n",
                    };
                    if existing {
                        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                        std::fs::write(&path, before).unwrap();
                    }
                    let ctx = make_linux_context(empty_config(home.path().to_path_buf()))
                        .with_home(home.path().to_path_buf())
                        .with_dry_run(dry_run);
                    let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![
                        AgentSetting {
                            target,
                            key: "unmanaged".to_string(),
                            value: toml::Value::Boolean(false),
                        },
                        AgentSetting {
                            target,
                            key: "model".to_string(),
                            value: toml::Value::String("base".to_string()),
                        },
                        AgentSetting {
                            target,
                            key: "model".to_string(),
                            value: toml::Value::String("overlay".to_string()),
                        },
                    ]));

                    let error = task
                        .run(&ctx)
                        .expect_err("conflicting desired state must fail");
                    assert!(error.to_string().contains("conflicting desired values"));
                    assert!(
                        error
                            .to_string()
                            .contains(&format!("{}:model", target.name()))
                    );
                    if existing {
                        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
                    } else {
                        assert!(!path.parent().unwrap().exists());
                    }
                }
            }
        }
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
