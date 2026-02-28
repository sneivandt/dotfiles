//! Windows registry entry configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::toml_loader;

/// A Windows registry entry.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Registry key path (e.g., "HKCU:\Console").
    pub key_path: String,
    /// Value name.
    pub value_name: String,
    /// Value data.
    pub value_data: String,
}

/// TOML registry section with path and values.
#[derive(Debug, Deserialize)]
struct RegistrySection {
    path: String,
    values: HashMap<String, toml::Value>,
}

/// Load registry settings from registry.toml.
///
/// Each top-level section contains a `path` field (registry key path)
/// and a `values` table with key-value pairs. Values are converted to strings.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path) -> Result<Vec<RegistryEntry>> {
    let config: HashMap<String, RegistrySection> = toml_loader::load_config(path)?;

    Ok(config
        .into_iter()
        .flat_map(|(_, section)| {
            let key_path = section.path;
            section.values.into_iter().map(move |(name, value)| {
                let value_data = value_to_string(&value);
                RegistryEntry {
                    key_path: key_path.clone(),
                    value_name: name,
                    value_data,
                }
            })
        })
        .collect())
}

/// Convert a TOML value to a string representation for registry data.
fn value_to_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => if *b { "1" } else { "0" }.to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn load_registry_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(
            &path,
            "[console]\npath = \"HKCU:\\\\Console\"\n[console.values]\nFontSize = 14\nCursorSize = 100\n",
        )
        .unwrap();

        let entries = load(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.key_path == "HKCU:\\Console"));
        let font_size = entries
            .iter()
            .find(|e| e.value_name == "FontSize")
            .expect("FontSize entry");
        assert_eq!(font_size.value_data, "14");
        let cursor_size = entries
            .iter()
            .find(|e| e.value_name == "CursorSize")
            .expect("CursorSize entry");
        assert_eq!(cursor_size.value_data, "100");
    }

    #[test]
    fn load_multiple_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(
            &path,
            "[console]\npath = \"HKCU:\\\\Console\"\n[console.values]\nFontSize = 14\n\n[explorer]\npath = \"HKCU:\\\\Explorer\"\n[explorer.values]\nShowHidden = 1\n",
        )
        .unwrap();

        let entries = load(&path).unwrap();
        assert_eq!(entries.len(), 2);

        // Check that both entries exist (order is not guaranteed with HashMap)
        let console_entry = entries.iter().find(|e| e.key_path == "HKCU:\\Console");
        let explorer_entry = entries.iter().find(|e| e.key_path == "HKCU:\\Explorer");

        assert!(console_entry.is_some(), "should have console entry");
        assert!(explorer_entry.is_some(), "should have explorer entry");

        let console = console_entry.unwrap();
        assert_eq!(console.value_name, "FontSize");
        assert_eq!(console.value_data, "14");

        let explorer = explorer_entry.unwrap();
        assert_eq!(explorer.value_name, "ShowHidden");
        assert_eq!(explorer.value_data, "1");
    }

    #[test]
    fn load_empty_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "").unwrap();
        let entries = load(&path).unwrap();
        assert!(entries.is_empty(), "empty file should produce empty list");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let entries = load(&dir.path().join("nope.toml")).unwrap();
        assert!(entries.is_empty());
    }
}
