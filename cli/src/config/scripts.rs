//! Script entry configuration for overlay custom tasks.
//!
//! Scripts are defined in `scripts.toml` inside an overlay repository.
//! Each script entry specifies a name, a path to a script file relative to
//! the overlay root, and an optional description.
use serde::Deserialize;
use std::path::PathBuf;

use super::config_section;

/// A custom script entry loaded from `scripts.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptEntry {
    /// Human-readable name for this script task.
    pub name: String,
    /// Path to the script file, relative to the overlay root.
    pub path: String,
    /// Optional description shown in log output.
    #[serde(default)]
    pub description: Option<String>,
}

config_section!(field: "scripts", ty: ScriptEntry);

/// Resolve the absolute path for a script entry relative to the overlay root.
#[must_use]
pub fn resolve_script_path(entry: &ScriptEntry, overlay_root: &std::path::Path) -> PathBuf {
    overlay_root.join(&entry.path)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::write_temp_toml;

    #[test]
    fn load_scripts_from_toml() {
        let content = r#"
[base]
scripts = [
    { name = "Setup database", path = "scripts/setup-db.ps1", description = "Configure local database connections" },
    { name = "Setup editor", path = "scripts/editor.ps1" },
]
"#;
        let (_dir, path) = write_temp_toml(content);
        let entries = load(&path, &[Category::Base]).expect("load should succeed");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Setup database");
        assert_eq!(entries[0].path, "scripts/setup-db.ps1");
        assert_eq!(
            entries[0].description.as_deref(),
            Some("Configure local database connections")
        );
        assert_eq!(entries[1].name, "Setup editor");
        assert!(entries[1].description.is_none());
    }

    #[test]
    fn load_empty_file_returns_empty() {
        let (_dir, path) = write_temp_toml("");
        let entries = load(&path, &[Category::Base]).expect("load should succeed");
        assert!(entries.is_empty());
    }

    #[test]
    fn load_missing_file_returns_empty() {
        crate::config::test_helpers::assert_load_missing_returns_empty(load);
    }

    #[test]
    fn resolve_script_path_joins_overlay_root() {
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "scripts/test.ps1".to_string(),
            description: None,
        };
        let resolved = resolve_script_path(&entry, std::path::Path::new("/overlay"));
        assert_eq!(resolved, PathBuf::from("/overlay/scripts/test.ps1"));
    }

    #[test]
    fn category_filtering_works() {
        let content = r#"
[base]
scripts = [
    { name = "Base script", path = "scripts/base.sh" },
]

[desktop]
scripts = [
    { name = "Desktop script", path = "scripts/desktop.sh" },
]
"#;
        let (_dir, path) = write_temp_toml(content);
        let entries = load(&path, &[Category::Base]).expect("load should succeed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Base script");
    }
}
