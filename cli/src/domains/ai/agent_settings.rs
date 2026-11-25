//! Task: configure supported agent harness settings.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting, validate};
use crate::domains::ai::resources::agent_settings::{
    AgentSettingResource, AgentSettingsDocument, SettingsFormat,
};
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};

crate::engine::config_task! {
    /// Configure agent harness settings from `agent-settings.toml`.
    ///
    /// Managed keys in each user settings document converge in one transaction
    /// without disturbing unmanaged keys. Resolved document access claims serialize
    /// aliases while independent documents can overlap. Contradictory values
    /// for the same harness/key are rejected before any document is changed.
    pub(crate) struct ConfigureAgentSettings(Vec<AgentSetting>);
}

const NAME: &str = "Agent settings";

impl ConfigureAgentSettings {
    fn resource(setting: &AgentSetting, ctx: &Context) -> Result<AgentSettingResource> {
        let (format, path) = target_document(setting.target, ctx.home(), ctx.env().as_ref())?;
        Ok(AgentSettingResource::new(
            setting.target.name().to_string(),
            setting.key.clone(),
            setting.value.clone(),
            format,
            path,
        ))
    }
}

fn target_document(
    target: AgentHarness,
    home: &Path,
    env: &dyn crate::infra::env::Env,
) -> Result<(SettingsFormat, PathBuf)> {
    Ok(match target {
        AgentHarness::Copilot => (
            SettingsFormat::Json,
            home.join(".copilot").join("settings.json"),
        ),
        AgentHarness::Codex => {
            let directory = match env.var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
                Some(value) => {
                    let path = PathBuf::from(value);
                    anyhow::ensure!(path.is_absolute(), "CODEX_HOME must be an absolute path");
                    path
                }
                None => home.join(".codex"),
            };
            (SettingsFormat::Toml, directory.join("config.toml"))
        }
    })
}

impl Task for ConfigureAgentSettings {
    task_metadata! {
        name: NAME,
        selector: "agent-settings",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings = self.config.get();
        if let Some(conflict) = validate(settings)
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::infra::config::Severity::Error)
        {
            anyhow::bail!("{}: {}", conflict.item, conflict.message);
        }
        let resources = settings
            .iter()
            .map(|setting| Self::resource(setting, ctx))
            .collect::<Result<Vec<_>>>()?;
        run_resource_task(
            ctx,
            AgentSettingsDocument::group(resources)?,
            &ProcessOpts::strict("configure"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ai::config::agent_settings::{AgentHarness, AgentSetting};
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::infra::env::MapEnv;
    use crate::test_helpers::{empty_config, task_batch};
    use std::path::PathBuf;

    fn make_linux_context(root: PathBuf, overlay: Option<PathBuf>) -> Context {
        crate::test_helpers::make_linux_context(root, overlay).with_env(MapEnv::new().into_handle())
    }

    #[test]
    fn codex_home_override_preserves_default_config_and_converges() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let custom = fixture.path().join("custom codex");
        let default_path = home.join(".codex/config.toml");
        std::fs::create_dir_all(default_path.parent().unwrap()).unwrap();
        std::fs::write(&default_path, "unmanaged = true\n").unwrap();
        let ctx = make_linux_context(fixture.path().into(), None)
            .with_home(home)
            .with_env(MapEnv::new().with("CODEX_HOME", &custom).into_handle());
        let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![AgentSetting {
            target: AgentHarness::Codex,
            key: "approval_policy".into(),
            value: "on-request".into(),
        }]));
        assert_eq!(
            task_batch(&task.run(&ctx.with_dry_run(true)).unwrap()).changed_count(),
            1
        );
        assert!(!custom.exists());
        assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
        let actual: toml::Table =
            toml::from_str(&std::fs::read_to_string(custom.join("config.toml")).unwrap()).unwrap();
        assert_eq!(actual["approval_policy"].as_str(), Some("on-request"));
        assert_eq!(task_batch(&task.run(&ctx).unwrap()).already_ok_count(), 1);
        assert_eq!(
            std::fs::read_to_string(default_path).unwrap(),
            "unmanaged = true\n"
        );
    }

    #[test]
    fn relative_codex_home_fails_before_any_harness_write_and_empty_uses_default() {
        let home = tempfile::tempdir().unwrap();
        let task = ConfigureAgentSettings::new(ConfigHandle::new(
            [AgentHarness::Copilot, AgentHarness::Codex]
                .map(|target| AgentSetting {
                    target,
                    key: "model".into(),
                    value: "fixture".into(),
                })
                .into(),
        ));
        let ctx = make_linux_context(home.path().into(), None).with_home(home.path().into());
        for dry_run in [false, true] {
            let error = task
                .run(
                    &ctx.with_dry_run(dry_run)
                        .with_env(MapEnv::new().with("CODEX_HOME", "relative").into_handle()),
                )
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("CODEX_HOME must be an absolute path")
            );
            assert!(!home.path().join(".copilot").exists());
            assert!(!home.path().join(".codex").exists());
        }
        let result = task
            .run(&ctx.with_env(MapEnv::new().with("CODEX_HOME", "").into_handle()))
            .unwrap();
        assert_eq!(task_batch(&result).changed_count(), 2);
        assert!(home.path().join(".codex/config.toml").is_file());
    }

    #[test]
    fn run_is_not_applicable_without_settings() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        let result = ConfigureAgentSettings::new(ConfigHandle::new(vec![]))
            .run(&ctx)
            .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_with_settings_converges() {
        let dir = tempfile::tempdir().unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config.root.clone(), config.overlay)
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
        assert_eq!(task_batch(&result).changed_count(), 2);

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
        assert_eq!(task_batch(&repeated).already_ok_count(), 2);
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
            let ctx = make_linux_context(home.path().to_path_buf(), None)
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
                    let (_, path) = target_document(target, home.path(), &MapEnv::new()).unwrap();
                    let before = match target {
                        AgentHarness::Copilot => "{\"model\":\"original\",\"unmanaged\":true}\n",
                        AgentHarness::Codex => "model = \"original\"\nunmanaged = true\n",
                    };
                    if existing {
                        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                        std::fs::write(&path, before).unwrap();
                    }
                    let ctx = make_linux_context(home.path().to_path_buf(), None)
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
    fn failed_setting_preserves_the_entire_document_and_retry_converges() {
        for target in [AgentHarness::Copilot, AgentHarness::Codex] {
            let home = tempfile::tempdir_in(".").unwrap();
            let (_, path) = target_document(target, home.path(), &MapEnv::new()).unwrap();
            let before = match target {
                AgentHarness::Copilot => r#"{"model":"old","footer":false,"unmanaged":"keep"}"#,
                AgentHarness::Codex => "model = \"old\"\nfooter = false\nunmanaged = \"keep\"\n",
            };
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, before).unwrap();
            let ctx = make_linux_context(home.path().into(), None)
                .with_home(home.path().into())
                .with_parallel(false);
            let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![
                AgentSetting {
                    target,
                    key: "model".into(),
                    value: toml::Value::String("new".into()),
                },
                AgentSetting {
                    target,
                    key: "footer.showBranch".into(),
                    value: true.into(),
                },
                AgentSetting {
                    target,
                    key: "footer.showQuota".into(),
                    value: false.into(),
                },
            ]));

            for dry_run in [true, false] {
                let error = task
                    .run(&ctx.with_dry_run(dry_run))
                    .expect_err("incompatible parent must stop the batch");
                assert!(
                    format!("{error:#}").contains("footer.showBranch"),
                    "{error:#}"
                );
                let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
                assert_eq!(report.stats().changed_count(), 0);
                assert_eq!(report.stats().failed_count(), 1);
                assert_eq!(report.not_attempted_count(), 0);
                assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
            }
            let contents = std::fs::read_to_string(&path).unwrap();
            assert_eq!(contents, before);
            match target {
                AgentHarness::Copilot => {
                    let mut document: serde_json::Value = serde_json::from_str(&contents).unwrap();
                    assert_eq!(
                        document,
                        serde_json::json!({"model":"old","footer":false,"unmanaged":"keep"})
                    );
                    document["footer"] = serde_json::json!({});
                    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
                }
                AgentHarness::Codex => {
                    let mut document: toml::Table = toml::from_str(&contents).unwrap();
                    assert_eq!(document["model"].as_str(), Some("old"));
                    assert_eq!(document["footer"].as_bool(), Some(false));
                    assert_eq!(document["unmanaged"].as_str(), Some("keep"));
                    assert_eq!(document.len(), 3);
                    document.remove("footer");
                    std::fs::write(&path, toml::to_string(&document).unwrap()).unwrap();
                }
            }

            let result = task.run(&ctx).expect("retry after repairing parent value");
            assert_eq!(task_batch(&result).changed_count(), 1);
            assert_eq!(task_batch(&result).already_ok_count(), 0);
            let converged = std::fs::read(&path).unwrap();
            let repeated = task.run(&ctx).expect("repeat converged settings");
            assert_eq!(task_batch(&repeated).changed_count(), 0);
            assert_eq!(task_batch(&repeated).already_ok_count(), 1);
            assert_eq!(std::fs::read(&path).unwrap(), converged);
        }
    }

    #[test]
    fn concurrent_batches_preserve_all_keys_in_shared_documents() {
        let home = tempfile::tempdir().unwrap();
        let ctx = make_linux_context(home.path().into(), None)
            .with_home(home.path().into())
            .with_parallel(true);
        let tasks: Vec<_> = (0..2)
            .map(|batch| {
                let settings = (0..16)
                    .flat_map(|key| {
                        [AgentHarness::Copilot, AgentHarness::Codex].map(|target| AgentSetting {
                            target,
                            key: format!("fixture.batch{batch}.key{key}"),
                            value: toml::Value::Integer(key),
                        })
                    })
                    .collect();
                ConfigureAgentSettings::new(ConfigHandle::new(settings))
            })
            .collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = tasks
                .iter()
                .map(|task| {
                    let ctx = &ctx;
                    scope.spawn(move || task.run(ctx))
                })
                .collect();
            for handle in handles {
                assert_eq!(
                    task_batch(&handle.join().unwrap().unwrap()).changed_count(),
                    2
                );
            }
        });
        for task in tasks {
            let result = task.run(&ctx).unwrap();
            assert_eq!(task_batch(&result).already_ok_count(), 2);
        }
    }

    #[test]
    fn invalid_desired_settings_fail_before_any_document_is_changed() {
        for invalid in [
            AgentSetting {
                target: AgentHarness::Copilot,
                key: "footer".into(),
                value: toml::from_str::<toml::Table>("showBranch = false")
                    .unwrap()
                    .into(),
            },
            AgentSetting {
                target: AgentHarness::Copilot,
                key: "another".into(),
                value: toml::Value::Float(f64::INFINITY),
            },
        ] {
            let home = tempfile::tempdir().unwrap();
            let ctx = make_linux_context(home.path().into(), None).with_home(home.path().into());
            let task = ConfigureAgentSettings::new(ConfigHandle::new(vec![
                AgentSetting {
                    target: AgentHarness::Copilot,
                    key: "footer.showBranch".into(),
                    value: true.into(),
                },
                invalid,
            ]));
            assert!(task.run(&ctx).is_err());
            assert!(!home.path().join(".copilot").exists());
        }
    }

    #[test]
    fn malformed_paths_reject_the_whole_batch_in_preview_and_apply() {
        for key in ["", " ", ".a", "a.", "a..b"] {
            for dry_run in [false, true] {
                let home = tempfile::tempdir().unwrap();
                let path = home.path().join(".copilot/settings.json");
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "{\"unmanaged\":true}\n").unwrap();
                let ctx = make_linux_context(home.path().into(), None)
                    .with_home(home.path().into())
                    .with_dry_run(dry_run);
                let settings = ["valid", key].map(|key| AgentSetting {
                    target: AgentHarness::Copilot,
                    key: key.into(),
                    value: true.into(),
                });
                assert!(
                    ConfigureAgentSettings::new(ConfigHandle::new(settings.into()))
                        .run(&ctx)
                        .is_err()
                );
                assert_eq!(
                    std::fs::read_to_string(path).unwrap(),
                    "{\"unmanaged\":true}\n"
                );
            }
        }
    }
}
