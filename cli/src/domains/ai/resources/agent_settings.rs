//! Agent harness settings resource.
//!
//! Manages individual keys inside JSON or TOML settings documents. Each
//! resource owns one dot-separated key path and writes only that key, preserving
//! unmanaged preferences and integration configuration.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde_json::{Map, Value};

use crate::domains::ai::config::agent_settings::value_is_finite;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Serialization format of an agent settings document.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsFormat {
    /// JSON object, used by GitHub Copilot CLI.
    Json,
    /// TOML table, used by `OpenAI` Codex.
    Toml,
}

/// A single key within an agent settings document.
#[derive(Debug)]
pub struct AgentSettingResource {
    target: String,
    key: String,
    desired_value: toml::Value,
    format: SettingsFormat,
    path: PathBuf,
}

impl AgentSettingResource {
    /// Create a new agent settings resource.
    #[must_use]
    pub const fn new(
        target: String,
        key: String,
        desired_value: toml::Value,
        format: SettingsFormat,
        path: PathBuf,
    ) -> Self {
        Self {
            target,
            key,
            desired_value,
            format,
            path,
        }
    }

    fn read_document_contents(&self) -> Result<String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(contents),
            Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => {
                Err(anyhow::Error::from(error).context(format!("reading {}", self.path.display())))
            }
        }
    }

    fn read_json_document(&self) -> Result<Value> {
        let contents = self.read_document_contents()?;
        if contents.trim().is_empty() {
            return Ok(Value::Object(Map::new()));
        }
        serde_json::from_str::<Map<String, Value>>(&contents)
            .map(Value::Object)
            .with_context(|| format!("parsing {}", self.path.display()))
    }

    fn read_toml_document(&self) -> Result<toml::Table> {
        let contents = self.read_document_contents()?;
        if contents.trim().is_empty() {
            return Ok(toml::Table::new());
        }
        toml::from_str(&contents).with_context(|| format!("parsing {}", self.path.display()))
    }

    fn current_json_value<'document>(
        &self,
        document: &'document Value,
    ) -> Option<&'document Value> {
        self.key
            .split('.')
            .try_fold(document, |node, segment| node.as_object()?.get(segment))
    }

    fn current_toml_value<'document>(
        &self,
        document: &'document toml::Table,
    ) -> Option<&'document toml::Value> {
        let mut segments = self.key.split('.');
        let node = document.get(segments.next()?)?;
        segments.try_fold(node, |node, segment| node.as_table()?.get(segment))
    }

    fn state_from_json_document(&self, document: &Value) -> ResourceState {
        let desired = toml_value_to_json(&self.desired_value);
        match self.current_json_value(document) {
            Some(current) if *current == desired => ResourceState::Correct,
            Some(current) => ResourceState::Incorrect {
                current: current.to_string(),
            },
            None => ResourceState::Missing,
        }
    }

    fn state_from_toml_document(&self, document: &toml::Table) -> ResourceState {
        match self.current_toml_value(document) {
            Some(current) if *current == self.desired_value => ResourceState::Correct,
            Some(current) => ResourceState::Incorrect {
                current: current.to_string(),
            },
            None => ResourceState::Missing,
        }
    }

    fn set_in_json_document(&self, document: &mut Value) -> Result<()> {
        let segments: Vec<&str> = self.key.split('.').collect();
        let Some((last, parents)) = segments.split_last() else {
            return Ok(());
        };

        let mut node = document;
        for segment in parents {
            let object = node
                .as_object_mut()
                .with_context(|| format!("settings key '{}' is not a JSON object", self.key))?;
            node = object
                .entry((*segment).to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !node.is_object() {
                anyhow::bail!(
                    "cannot set '{}': '{segment}' is not a JSON object",
                    self.key
                );
            }
        }

        let object = node
            .as_object_mut()
            .with_context(|| format!("settings key '{}' is not a JSON object", self.key))?;
        object.insert((*last).to_string(), toml_value_to_json(&self.desired_value));
        Ok(())
    }

    fn set_in_toml_document(&self, document: &mut toml::Table) -> Result<()> {
        let segments: Vec<&str> = self.key.split('.').collect();
        let Some((last, parents)) = segments.split_last() else {
            return Ok(());
        };

        let mut table = document;
        for segment in parents {
            let node = table
                .entry((*segment).to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            table = node.as_table_mut().with_context(|| {
                format!("cannot set '{}': '{segment}' is not a TOML table", self.key)
            })?;
        }
        table.insert((*last).to_string(), self.desired_value.clone());
        Ok(())
    }
}

impl Resource for AgentSettingResource {
    fn description(&self) -> String {
        format!("{}:{} = {}", self.target, self.key, self.desired_value)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if !value_is_finite(&self.desired_value) {
            return Err(anyhow::anyhow!("settings value contains NaN or infinity").into());
        }
        let mut serialized = match self.format {
            SettingsFormat::Json => {
                let mut document = self.read_json_document()?;
                self.set_in_json_document(&mut document)?;
                serde_json::to_string_pretty(&document)
                    .with_context(|| format!("serializing {}", self.path.display()))?
            }
            SettingsFormat::Toml => {
                let mut document = self.read_toml_document()?;
                self.set_in_toml_document(&mut document)?;
                toml::to_string_pretty(&document)
                    .with_context(|| format!("serializing {}", self.path.display()))?
            }
        };
        if !serialized.ends_with('\n') {
            serialized.push('\n');
        }
        // Preserve an existing redirection while publishing a complete document
        // at its referent. Broken links fail rather than being replaced silently.
        let path = match crate::infra::fs::symlink_metadata_optional(
            &self.path,
            "inspect settings destination",
        )? {
            Some(metadata) if metadata.is_symlink() => crate::infra::fs::canonicalize(&self.path)?,
            _ => self.path.clone(),
        };
        crate::infra::fs::write_atomic(&path, serialized)?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for AgentSettingResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if !value_is_finite(&self.desired_value) {
            return Ok(ResourceState::Invalid {
                reason: "settings value contains NaN or infinity".into(),
            });
        }
        match self.format {
            SettingsFormat::Json => {
                let document = self.read_json_document()?;
                Ok(self.state_from_json_document(&document))
            }
            SettingsFormat::Toml => {
                let document = self.read_toml_document()?;
                Ok(self.state_from_toml_document(&document))
            }
        }
    }
}

fn toml_value_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value.clone()),
        toml::Value::Integer(value) => Value::Number((*value).into()),
        toml::Value::Float(value) => {
            serde_json::Number::from_f64(*value).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(value) => Value::Bool(*value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_value_to_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, nested)| (key.clone(), toml_value_to_json(nested)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_resource(key: &str, value: toml::Value) -> AgentSettingResource {
        AgentSettingResource::new(
            "copilot".to_string(),
            key.to_string(),
            value,
            SettingsFormat::Json,
            PathBuf::from("/tmp/settings.json"),
        )
    }

    fn toml_resource(key: &str, value: toml::Value) -> AgentSettingResource {
        AgentSettingResource::new(
            "codex".to_string(),
            key.to_string(),
            value,
            SettingsFormat::Toml,
            PathBuf::from("/tmp/config.toml"),
        )
    }

    #[test]
    fn description_includes_target() {
        let resource = json_resource("model", toml::Value::String("claude-opus-4.8".to_string()));
        assert_eq!(
            resource.description(),
            "copilot:model = \"claude-opus-4.8\""
        );
    }

    #[test]
    fn json_state_reads_nested_path() {
        let document = serde_json::json!({ "footer": { "showBranch": true } });
        let resource = json_resource("footer.showBranch", toml::Value::Boolean(true));
        assert_eq!(
            resource.state_from_json_document(&document),
            ResourceState::Correct
        );

        let missing = json_resource("footer.showQuota", toml::Value::Boolean(false));
        assert_eq!(
            missing.state_from_json_document(&document),
            ResourceState::Missing
        );
    }

    #[test]
    fn json_set_preserves_nested_siblings() {
        let mut document = serde_json::json!({ "footer": { "showQuota": false } });
        let resource = json_resource("footer.showBranch", toml::Value::Boolean(true));
        resource.set_in_json_document(&mut document).unwrap();
        assert_eq!(document["footer"]["showQuota"], serde_json::json!(false));
        assert_eq!(document["footer"]["showBranch"], serde_json::json!(true));
    }

    #[test]
    fn json_set_errors_when_intermediate_is_not_object() {
        let mut document = serde_json::json!({ "footer": 42 });
        let resource = json_resource("footer.showBranch", toml::Value::Boolean(true));
        assert!(resource.set_in_json_document(&mut document).is_err());
    }

    #[test]
    fn non_object_json_documents_are_rejected_without_discarding_content() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("settings.json");
        let resource = AgentSettingResource::new(
            "copilot".to_string(),
            "model".to_string(),
            toml::Value::String("fixture-model".to_string()),
            SettingsFormat::Json,
            path.clone(),
        );

        for contents in [
            "null",
            "[]",
            r#"[{"keepMe":"value"}]"#,
            "true",
            "42",
            r#""text""#,
        ] {
            std::fs::write(&path, contents).unwrap();
            assert!(resource.current_state().is_err(), "{contents}");
            assert!(resource.apply().is_err(), "{contents}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn toml_state_and_set_preserve_sibling_tables() {
        let mut document: toml::Table = toml::from_str(
            r#"
model = "gpt-5.5"

[mcp_servers.example]
command = "example"
"#,
        )
        .unwrap();
        let resource = toml_resource(
            "model_reasoning_effort",
            toml::Value::String("high".to_string()),
        );
        assert_eq!(
            resource.state_from_toml_document(&document),
            ResourceState::Missing
        );
        resource.set_in_toml_document(&mut document).unwrap();
        assert_eq!(
            document["mcp_servers"]["example"]["command"].as_str(),
            Some("example")
        );
        assert_eq!(
            resource.state_from_toml_document(&document),
            ResourceState::Correct
        );
    }

    #[test]
    fn toml_set_errors_when_intermediate_is_not_table() {
        let mut document: toml::Table = toml::from_str("tui = false\n").unwrap();
        let resource = toml_resource("tui.theme", toml::Value::String("default".to_string()));
        assert!(resource.set_in_toml_document(&mut document).is_err());
    }

    #[test]
    fn missing_and_empty_documents_converge_in_both_formats() {
        for format in [SettingsFormat::Json, SettingsFormat::Toml] {
            for contents in [None, Some(""), Some(" \n\t\n")] {
                let dir = tempfile::tempdir_in(".").unwrap();
                let path = dir.path().join("settings");
                if let Some(contents) = contents {
                    std::fs::write(&path, contents).unwrap();
                }
                let resource = AgentSettingResource::new(
                    "fixture".to_string(),
                    "nested.setting".to_string(),
                    toml::Value::Boolean(true),
                    format,
                    path,
                );
                assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
                assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
                assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            }
        }
    }

    #[test]
    fn apply_json_writes_file_and_preserves_unmanaged_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\n  \"keepMe\": \"value\"\n}\n").unwrap();
        let resource = AgentSettingResource::new(
            "copilot".to_string(),
            "model".to_string(),
            toml::Value::String("claude-opus-4.8".to_string()),
            SettingsFormat::Json,
            path.clone(),
        );

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        let contents = std::fs::read_to_string(&path).unwrap();
        let document: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(document["keepMe"], serde_json::json!("value"));
        assert_eq!(document["model"], serde_json::json!("claude-opus-4.8"));
        assert_eq!(
            contents,
            serde_json::to_string_pretty(&document).unwrap() + "\n"
        );
        resource.apply().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }

    #[test]
    fn apply_toml_writes_file_and_preserves_unmanaged_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".codex").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "model = \"gpt-5.5\"\n\n[mcp_servers.example]\ncommand = \"example\"\n",
        )
        .unwrap();
        let resource = AgentSettingResource::new(
            "codex".to_string(),
            "model_reasoning_effort".to_string(),
            toml::Value::String("high".to_string()),
            SettingsFormat::Toml,
            path.clone(),
        );

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        let contents = std::fs::read_to_string(&path).unwrap();
        let document: toml::Table = toml::from_str(&contents).unwrap();
        assert_eq!(document["model"].as_str(), Some("gpt-5.5"));
        assert_eq!(
            document["mcp_servers"]["example"]["command"].as_str(),
            Some("example")
        );
        assert!(contents.ends_with('\n') && !contents.ends_with("\n\n"));
        resource.apply().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }

    #[test]
    fn invalid_documents_return_errors() {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("settings.json");
        std::fs::write(&json_path, "{ not json").unwrap();
        let json = AgentSettingResource::new(
            "copilot".to_string(),
            "model".to_string(),
            toml::Value::String("gpt-5.6-sol".to_string()),
            SettingsFormat::Json,
            json_path.clone(),
        );
        assert!(json.current_state().is_err());
        assert!(json.apply().is_err());
        assert_eq!(std::fs::read_to_string(json_path).unwrap(), "{ not json");

        let toml_path = dir.path().join("config.toml");
        std::fs::write(&toml_path, "[not valid").unwrap();
        let toml = AgentSettingResource::new(
            "codex".to_string(),
            "model".to_string(),
            toml::Value::String("gpt-5.6-sol".to_string()),
            SettingsFormat::Toml,
            toml_path.clone(),
        );
        assert!(toml.current_state().is_err());
        assert!(toml.apply().is_err());
        assert_eq!(std::fs::read_to_string(toml_path).unwrap(), "[not valid");
    }

    #[test]
    fn invalid_numeric_values_cannot_overwrite_documents_in_either_format() {
        for format in [SettingsFormat::Json, SettingsFormat::Toml] {
            for value in [
                toml::Value::Float(f64::NAN),
                toml::Value::Array(vec![toml::Value::Float(f64::INFINITY)]),
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("settings");
                let before = match format {
                    SettingsFormat::Json => "{\"unmanaged\":true}",
                    SettingsFormat::Toml => "unmanaged = true\n",
                };
                std::fs::write(&path, before).unwrap();
                let resource = AgentSettingResource::new(
                    "fixture".into(),
                    "managed".into(),
                    value,
                    format,
                    path.clone(),
                );
                assert!(matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Invalid { .. }
                ));
                assert!(resource.apply().is_err());
                assert_eq!(std::fs::read_to_string(path).unwrap(), before);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_settings_updates_preserve_symlink_redirections_and_access_modes() {
        use std::os::unix::fs::PermissionsExt as _;
        for format in [SettingsFormat::Json, SettingsFormat::Toml] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("actual-settings");
            let path = dir.path().join("settings");
            let before = match format {
                SettingsFormat::Json => "{\"unmanaged\":true}",
                SettingsFormat::Toml => "unmanaged = true\n",
            };
            std::fs::write(&target, before).unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
            std::os::unix::fs::symlink("actual-settings", &path).unwrap();
            let resource = AgentSettingResource::new(
                "fixture".into(),
                "managed".into(),
                true.into(),
                format,
                path.clone(),
            );
            resource.apply().unwrap();
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(
                std::fs::read_link(path).unwrap(),
                PathBuf::from("actual-settings")
            );
            assert_eq!(
                target.metadata().unwrap().permissions().mode() & 0o777,
                0o640
            );
            let contents = std::fs::read_to_string(&target).unwrap();
            assert!(contents.contains("unmanaged"));
            assert!(contents.contains("managed"));
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_staged_write_preserves_existing_document() {
        const CHILD_PATH: &str = "DOTFILES_TEST_LIMITED_SETTINGS_PATH";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let format = if std::env::var_os("DOTFILES_TEST_SETTINGS_TOML").is_some() {
                SettingsFormat::Toml
            } else {
                SettingsFormat::Json
            };
            let resource = AgentSettingResource::new(
                "fixture".into(),
                "managed".into(),
                toml::Value::String("x".repeat(65536)),
                format,
                path.into(),
            );
            assert!(
                resource.apply().is_err(),
                "file size limit must reject the replacement"
            );
            return;
        }
        for format in [SettingsFormat::Json, SettingsFormat::Toml] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings");
            let before = match format {
                SettingsFormat::Json => "{\"unmanaged\":true}",
                SettingsFormat::Toml => "unmanaged = true\n",
            };
            std::fs::write(&path, before).unwrap();
            let mut child = std::process::Command::new("sh");
            child.args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "limited-settings-write"])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "domains::ai::resources::agent_settings::tests::failed_staged_write_preserves_existing_document"])
                // The file-size limit would truncate the instrumented child's profile.
                .env("LLVM_PROFILE_FILE", "/dev/null")
                .env(CHILD_PATH, &path);
            if format == SettingsFormat::Toml {
                child.env("DOTFILES_TEST_SETTINGS_TOML", "1");
            }
            let output = child.output().unwrap();
            assert!(output.status.success(), "limited writer failed: {output:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                1,
                "failed replacement must clean staging files"
            );
        }
    }
}
