//! Agent harness settings resource.
//!
//! Manages individual keys inside JSON or TOML settings documents. Each
//! resource owns one dot-separated key path and writes only that key, preserving
//! unmanaged preferences and integration configuration.

mod toml_document;

use toml_document::{set_editable_toml, toml_value_to_json};

mod json;

use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde_json::{Map, Value};

use crate::domains::ai::config::agent_settings::value_is_finite;
use crate::engine::{
    AccessClaim, IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState,
};

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

    #[cfg(test)]
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
    fn access_claims(&self) -> Vec<AccessClaim> {
        vec![AccessClaim::exclusive(
            "agent-settings-document",
            self.path.as_os_str(),
        )]
    }

    fn description(&self) -> String {
        format!("{}:{} = {}", self.target, self.key, self.desired_value)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        Ok(apply_document(
            std::slice::from_ref(self),
            &document_path(&self.path)?,
        )?)
    }
}

impl IntrinsicState for AgentSettingResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if !value_is_finite(&self.desired_value) || invalid_key(&self.key) {
            return Ok(ResourceState::Invalid {
                reason: "settings key is empty/has an empty path segment, or value contains NaN or infinity".into(),
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

fn invalid_key(key: &str) -> bool {
    key.trim().is_empty() || key.split('.').any(str::is_empty)
}

fn document_path(path: &Path) -> Result<PathBuf> {
    if crate::infra::fs::symlink_metadata_optional(path, "inspect settings destination")?.is_some()
    {
        crate::infra::fs::canonicalize(path)
    } else {
        let parent = path.parent().context("settings document has no parent")?;
        Ok(document_path(parent)?.join(path.file_name().context("settings document has no name")?))
    }
}

fn read_baseline(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn apply_document(settings: &[AgentSettingResource], path: &Path) -> Result<ResourceChange> {
    let before = read_baseline(path)?;
    let Some(serialized) = planned_document(settings, path, before.as_deref())? else {
        return Ok(ResourceChange::AlreadyCorrect);
    };
    publish_document(settings, path, before.as_deref(), serialized)?;
    Ok(ResourceChange::Applied)
}

fn planned_document(
    settings: &[AgentSettingResource],
    path: &Path,
    before: Option<&[u8]>,
) -> Result<Option<String>> {
    let first = settings.first().context("settings document has no keys")?;
    for setting in settings {
        anyhow::ensure!(
            !invalid_key(&setting.key),
            "invalid settings key '{}'",
            setting.key
        );
        anyhow::ensure!(
            value_is_finite(&setting.desired_value),
            "settings value contains NaN or infinity"
        );
    }
    let contents = std::str::from_utf8(before.unwrap_or_default())?;
    let mut serialized = match first.format {
        SettingsFormat::Json => {
            let Some(serialized) = json::plan(contents, settings)? else {
                return Ok(None);
            };
            serialized
        }
        SettingsFormat::Toml => {
            let current: toml::Table = toml::from_str(contents)?;
            let mut changed = settings
                .iter()
                .filter(|setting| {
                    setting.current_toml_value(&current) != Some(&setting.desired_value)
                })
                .peekable();
            if changed.peek().is_none() {
                return Ok(None);
            }
            let mut document = contents.parse::<toml_edit::DocumentMut>()?;
            for setting in changed {
                set_editable_toml(&mut document, setting)?;
            }
            let serialized = document.to_string();
            let updated: toml::Table = toml::from_str(&serialized)
                .with_context(|| format!("checking serialized settings for {}", path.display()))?;
            for setting in settings {
                anyhow::ensure!(
                    setting.current_toml_value(&updated) == Some(&setting.desired_value),
                    "serialized settings for {} did not preserve the desired value of '{}'",
                    path.display(),
                    setting.key
                );
            }
            serialized
        }
    };
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    Ok(Some(serialized))
}

fn publish_document(
    settings: &[AgentSettingResource],
    path: &Path,
    before: Option<&[u8]>,
    serialized: String,
) -> Result<()> {
    crate::infra::fs::write_atomic_checked(path, serialized, || {
        anyhow::ensure!(
            read_baseline(path)?.as_deref() == before,
            "settings document {} changed during update; retry",
            path.display()
        );
        // Recheck each original spelling now, not against group-time evidence.
        let mut checked = HashSet::new();
        for setting in settings {
            if !checked.insert(setting.path.as_os_str()) {
                continue;
            }
            anyhow::ensure!(
                document_path(&setting.path)? == path,
                "settings destination changed during update; retry"
            );
        }
        Ok(())
    })
}

/// One transaction for all managed keys in a resolved settings document.
#[derive(Debug)]
pub struct AgentSettingsDocument {
    path: PathBuf,
    settings: Vec<AgentSettingResource>,
}

impl AgentSettingsDocument {
    /// Group keys by filesystem destination, rejecting incompatible document formats.
    ///
    /// # Errors
    ///
    /// Returns resolution errors or conflicting format errors before any writes.
    pub fn group(settings: Vec<AgentSettingResource>) -> Result<Vec<Self>> {
        Self::group_with(settings, document_path)
    }

    fn group_with(
        settings: Vec<AgentSettingResource>,
        mut resolve: impl FnMut(&Path) -> Result<PathBuf>,
    ) -> Result<Vec<Self>> {
        let mut documents = Vec::<Self>::new();
        // Path equality normalizes some spellings; only identical OS strings
        // are guaranteed to request the same filesystem lookup.
        let mut resolved = HashMap::new();
        for setting in settings {
            let path = match resolved.entry(setting.path.as_os_str().to_owned()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(resolve(&setting.path)?),
            };
            if let Some(document) = documents.iter_mut().find(|document| document.path == *path) {
                anyhow::ensure!(
                    document
                        .settings
                        .iter()
                        .all(|other| other.key != setting.key
                            || other.desired_value == setting.desired_value),
                    "conflicting values for settings key '{}' in {}",
                    setting.key,
                    path.display()
                );
                anyhow::ensure!(
                    document
                        .settings
                        .iter()
                        .all(|other| other.format == setting.format),
                    "incompatible settings formats share {}",
                    path.display()
                );
                document.settings.push(setting);
            } else {
                documents.push(Self {
                    path: path.clone(),
                    settings: vec![setting],
                });
            }
        }
        // A selected parent may replace an incompatible existing scalar. Apply
        // it before an agreeing descendant regardless of declaration order.
        for document in &mut documents {
            document
                .settings
                .sort_by_key(|setting| setting.key.matches('.').count());
        }
        Ok(documents)
    }
}

impl Resource for AgentSettingsDocument {
    fn access_claims(&self) -> Vec<AccessClaim> {
        vec![AccessClaim::exclusive(
            "agent-settings-document",
            self.path.as_os_str(),
        )]
    }

    fn description(&self) -> String {
        format!(
            "{} ({} managed keys)",
            self.path.display(),
            self.settings.len()
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        Ok(apply_document(&self.settings, &self.path)?)
    }
}

impl IntrinsicState for AgentSettingsDocument {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let before = read_baseline(&self.path)?;
        Ok(
            if planned_document(&self.settings, &self.path, before.as_deref())?.is_none() {
                ResourceState::Correct
            } else if before.is_none() {
                ResourceState::Missing
            } else {
                ResourceState::Incorrect {
                    current: "managed settings differ".into(),
                }
            },
        )
    }
}

#[cfg(test)]
mod tests;
