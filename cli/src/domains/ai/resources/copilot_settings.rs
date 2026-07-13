//! Copilot CLI settings resource.
//!
//! Manages individual keys inside a JSON settings document (such as
//! `~/.copilot/settings.json`).  Each resource owns a single dot-separated key
//! path: it reads the current document, compares the value at that path, and —
//! when applying — writes only that key back, leaving every other key intact.
//!
//! Writing only the managed keys is what makes this safe for *volatile* files
//! that the Copilot CLI itself rewrites at runtime (for example after `/model`
//! or `/theme`): unmanaged keys, login state, and plugin bookkeeping are
//! preserved.
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde_json::{Map, Value};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// A single key within a JSON settings document.
#[derive(Debug)]
pub struct CopilotSettingResource {
    /// Dot-separated key path (e.g. `"model"` or `"footer.showBranch"`).
    pub key: String,
    /// Desired JSON value for the key.
    pub desired_value: Value,
    /// Path to the JSON settings file (e.g. `~/.copilot/settings.json`).
    pub path: PathBuf,
}

impl CopilotSettingResource {
    /// Create a new Copilot settings resource.
    #[must_use]
    pub const fn new(key: String, desired_value: Value, path: PathBuf) -> Self {
        Self {
            key,
            desired_value,
            path,
        }
    }

    /// Read and parse the settings document, treating a missing or empty file
    /// as an empty JSON object.
    fn read_document(&self) -> Result<Value> {
        match std::fs::read_to_string(&self.path) {
            Ok(ref contents) if contents.trim().is_empty() => Ok(Value::Object(Map::new())),
            Ok(contents) => serde_json::from_str(&contents)
                .with_context(|| format!("parsing {}", self.path.display())),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("reading {}", self.path.display())))
            }
        }
    }

    /// Look up the current value at this resource's key path, if present.
    fn current_value<'doc>(&self, document: &'doc Value) -> Option<&'doc Value> {
        let mut node = document;
        for segment in self.key.split('.') {
            node = node.as_object()?.get(segment)?;
        }
        Some(node)
    }

    /// Compute state from an already-parsed document (testable without disk).
    fn state_from_document(&self, document: &Value) -> ResourceState {
        match self.current_value(document) {
            Some(current) if *current == self.desired_value => ResourceState::Correct,
            Some(current) => ResourceState::Incorrect {
                current: current.to_string(),
            },
            None => ResourceState::Missing,
        }
    }

    /// Set this resource's key path within a document, creating intermediate
    /// objects as needed.
    ///
    /// # Errors
    ///
    /// Returns an error if an intermediate node along the key path exists but
    /// is not a JSON object (so writing the key would discard sibling data).
    fn set_in_document(&self, document: &mut Value) -> Result<()> {
        let segments: Vec<&str> = self.key.split('.').collect();
        let Some((last, parents)) = segments.split_last() else {
            return Ok(());
        };

        if !document.is_object() {
            *document = Value::Object(Map::new());
        }

        let mut node = document;
        for segment in parents {
            let obj = node
                .as_object_mut()
                .with_context(|| format!("settings key '{}' is not a JSON object", self.key))?;
            node = obj
                .entry((*segment).to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !node.is_object() {
                anyhow::bail!(
                    "cannot set '{}': '{segment}' is not a JSON object",
                    self.key
                );
            }
        }

        let obj = node
            .as_object_mut()
            .with_context(|| format!("settings key '{}' is not a JSON object", self.key))?;
        obj.insert((*last).to_string(), self.desired_value.clone());
        Ok(())
    }
}

impl Resource for CopilotSettingResource {
    fn description(&self) -> String {
        format!("{} = {}", self.key, self.desired_value)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let mut document = self.read_document()?;
        self.set_in_document(&mut document)?;

        let mut serialized = serde_json::to_string_pretty(&document)
            .with_context(|| format!("serializing {}", self.path.display()))?;
        serialized.push('\n');

        crate::runtime::fs::write_with_parent(&self.path, serialized)?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for CopilotSettingResource {
    fn current_state(&self) -> Result<ResourceState> {
        let document = self.read_document()?;
        Ok(self.state_from_document(&document))
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

    fn resource(key: &str, value: Value) -> CopilotSettingResource {
        CopilotSettingResource::new(key.to_string(), value, PathBuf::from("/tmp/settings.json"))
    }

    #[test]
    fn description_format() {
        let r = resource("model", Value::String("claude-opus-4.8".to_string()));
        assert_eq!(r.description(), "model = \"claude-opus-4.8\"");
    }

    // ------------------------------------------------------------------
    // state_from_document
    // ------------------------------------------------------------------

    #[test]
    fn state_correct_when_value_matches() {
        let doc = serde_json::json!({ "model": "claude-opus-4.8" });
        let r = resource("model", Value::String("claude-opus-4.8".to_string()));
        assert_eq!(r.state_from_document(&doc), ResourceState::Correct);
    }

    #[test]
    fn state_missing_when_key_absent() {
        let doc = serde_json::json!({ "other": true });
        let r = resource("model", Value::String("x".to_string()));
        assert_eq!(r.state_from_document(&doc), ResourceState::Missing);
    }

    #[test]
    fn state_incorrect_when_value_differs() {
        let doc = serde_json::json!({ "beep": true });
        let r = resource("beep", Value::Bool(false));
        let state = r.state_from_document(&doc);
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "true"),
            "expected Incorrect(true), got {state:?}"
        );
    }

    #[test]
    fn state_reads_nested_path() {
        let doc = serde_json::json!({ "footer": { "showBranch": true } });
        let r = resource("footer.showBranch", Value::Bool(true));
        assert_eq!(r.state_from_document(&doc), ResourceState::Correct);

        let r_missing = resource("footer.showQuota", Value::Bool(false));
        assert_eq!(r_missing.state_from_document(&doc), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // set_in_document
    // ------------------------------------------------------------------

    #[test]
    fn set_creates_top_level_key_preserving_siblings() {
        let mut doc = serde_json::json!({ "existing": 1 });
        let r = resource("model", Value::String("x".to_string()));
        r.set_in_document(&mut doc).unwrap();
        assert_eq!(doc["existing"], serde_json::json!(1));
        assert_eq!(doc["model"], serde_json::json!("x"));
    }

    #[test]
    fn set_creates_nested_path() {
        let mut doc = Value::Object(Map::new());
        let r = resource("footer.showBranch", Value::Bool(true));
        r.set_in_document(&mut doc).unwrap();
        assert_eq!(doc["footer"]["showBranch"], serde_json::json!(true));
    }

    #[test]
    fn set_preserves_sibling_nested_keys() {
        let mut doc = serde_json::json!({ "footer": { "showQuota": false } });
        let r = resource("footer.showBranch", Value::Bool(true));
        r.set_in_document(&mut doc).unwrap();
        assert_eq!(doc["footer"]["showQuota"], serde_json::json!(false));
        assert_eq!(doc["footer"]["showBranch"], serde_json::json!(true));
    }

    #[test]
    fn set_errors_when_intermediate_is_not_object() {
        let mut doc = serde_json::json!({ "footer": 42 });
        let r = resource("footer.showBranch", Value::Bool(true));
        assert!(r.set_in_document(&mut doc).is_err());
    }

    // ------------------------------------------------------------------
    // apply / current_state round-trip
    // ------------------------------------------------------------------

    #[test]
    fn apply_writes_file_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        let r = CopilotSettingResource::new(
            "footer.showBranch".to_string(),
            Value::Bool(true),
            path.clone(),
        );

        assert_eq!(r.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(r.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(r.current_state().unwrap(), ResourceState::Correct);

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.ends_with('\n'), "file should end with newline");
    }

    #[test]
    fn apply_preserves_unmanaged_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\n  \"keepMe\": \"value\"\n}\n").unwrap();

        let r = CopilotSettingResource::new(
            "model".to_string(),
            Value::String("claude-opus-4.8".to_string()),
            path.clone(),
        );
        r.apply().unwrap();

        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["keepMe"], serde_json::json!("value"));
        assert_eq!(doc["model"], serde_json::json!("claude-opus-4.8"));
    }

    #[test]
    fn read_document_errors_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let r = CopilotSettingResource::new("model".to_string(), Value::Null, path);
        assert!(r.current_state().is_err());
    }
}
