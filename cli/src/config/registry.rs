//! Windows registry entry configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::toml_loader;

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

/// Valid Windows registry hive prefixes.
const VALID_HIVE_PREFIXES: &[&str] = &["HKCU:", "HKLM:", "HKCR:", "HKU:", "HKCC:"];

/// Validate registry entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[RegistryEntry],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    let mut v = Validator::new("registry.toml");
    v.warn_if(
        !entries.is_empty() && !platform.has_registry(),
        "registry entries",
        "registry entries defined but platform does not support the Windows registry",
    );
    v.check_each(entries, |e| &e.value_name, |e| {
        let upper = e.key_path.to_uppercase();
        let has_valid_hive = VALID_HIVE_PREFIXES
            .iter()
            .any(|prefix| upper.starts_with(prefix));
        vec![
            check(e.key_path.trim().is_empty(), "registry key path is empty"),
            check(e.value_name.trim().is_empty(), "registry value name is empty"),
            check(
                !has_valid_hive,
                "registry key path should start with a valid hive (HKCU:, HKLM:, HKCR:, HKU:, HKCC:)",
            ),
            check(
                has_valid_hive && !upper.starts_with("HKCU:"),
                "non-HKCU registry hive requires elevated privileges and may fail without admin rights",
            ),
        ]
    })
    .finish()
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

    #[test]
    fn validate_detects_invalid_hive() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "INVALID:\\Key".to_string(),
            value_name: "Test".to_string(),
            value_data: "1".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid hive"));
    }

    #[test]
    fn validate_detects_empty_key_path() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "  ".to_string(),
            value_name: "Test".to_string(),
            value_data: "1".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.iter().any(|w| w.message.contains("path is empty")),
            "should warn about empty key path: {warnings:?}"
        );
    }

    #[test]
    fn validate_detects_empty_value_name() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "  ".to_string(),
            value_data: "1".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("value name is empty")),
            "should warn about empty value name: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_non_hkcu_hive() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKLM:\\Software\\Test".to_string(),
            value_name: "Setting".to_string(),
            value_data: "1".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("elevated privileges")),
            "should warn about non-HKCU hive needing elevation: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_registry_on_non_windows() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("does not support")),
            "should warn about registry on non-Windows: {warnings:?}"
        );
    }

    #[test]
    fn validate_valid_hkcu_entry_produces_no_warnings() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "valid HKCU entry should produce no warnings: {warnings:?}"
        );
    }

    #[test]
    fn validate_empty_entries_produces_no_warnings() {
        use crate::platform::{Os, Platform};

        let warnings = validate(&[], Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "empty entries should produce no warnings"
        );
    }

    #[test]
    fn validate_case_insensitive_hive_prefix() {
        use crate::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "hkcu:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "lowercase hive prefix should be accepted: {warnings:?}"
        );
    }

    #[test]
    fn value_to_string_converts_types() {
        assert_eq!(
            value_to_string(&toml::Value::String("hello".into())),
            "hello"
        );
        assert_eq!(value_to_string(&toml::Value::Integer(42)), "42");
        assert_eq!(value_to_string(&toml::Value::Float(2.72)), "2.72");
        assert_eq!(value_to_string(&toml::Value::Boolean(true)), "1");
        assert_eq!(value_to_string(&toml::Value::Boolean(false)), "0");
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "[console\npath = \"HKCU:\\\\Test\"").unwrap();
        let result = load(&path);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_missing_path_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "[console]\n[console.values]\nKey = \"Value\"\n").unwrap();
        let result = load(&path);
        assert!(result.is_err(), "missing 'path' field should return error");
    }
}
