use anyhow::Result;
use std::path::Path;

use super::ini;

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

/// Load registry settings from registry.ini.
///
/// Section headers are registry key paths (e.g., `[HKCU:\Console]`).
/// Each key-value entry becomes a `RegistryEntry`. Inline comments are
/// stripped by the common KV parser.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path) -> Result<Vec<RegistryEntry>> {
    let sections = ini::parse_kv_sections(path)?;

    Ok(sections
        .into_iter()
        .flat_map(|section| {
            let key_path = section.header;
            section
                .entries
                .into_iter()
                .map(move |(name, value)| RegistryEntry {
                    key_path: key_path.clone(),
                    value_name: name,
                    value_data: value,
                })
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn load_registry_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.ini");
        std::fs::write(
            &path,
            "# comment\n[HKCU:\\Console]\nFontSize = 14 # size\nCursorSize = 100\n",
        )
        .unwrap();

        let entries = load(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key_path, "HKCU:\\Console");
        assert_eq!(entries[0].value_name, "FontSize");
        assert_eq!(entries[0].value_data, "14");
        assert_eq!(entries[1].value_name, "CursorSize");
        assert_eq!(entries[1].value_data, "100");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let entries = load(&dir.path().join("nope.ini")).unwrap();
        assert!(entries.is_empty());
    }
}
