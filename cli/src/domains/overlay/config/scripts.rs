//! Script entry configuration for overlay custom tasks.
//!
//! Scripts are defined in `scripts.toml` inside an overlay repository.
//! Each script entry specifies a name, a path to a script file relative to
//! the overlay root, and an optional description.
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

use crate::runtime::config_support::config_section;

/// A custom script entry loaded from `scripts.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
///
/// # Errors
///
/// Returns an error when the configured script path is absolute or contains
/// parent-directory traversal.
pub fn resolve_script_path(entry: &ScriptEntry, overlay_root: &Path) -> Result<PathBuf> {
    validate_relative_script_path(entry)?;
    Ok(overlay_root.join(&entry.path))
}

fn validate_relative_script_path(entry: &ScriptEntry) -> Result<()> {
    let path = Path::new(&entry.path);
    if path.as_os_str().is_empty() {
        bail!("script path for '{}' is empty", entry.name);
    }

    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                bail!("script path for '{}' must not contain '..'", entry.name);
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("script path for '{}' must be relative", entry.name);
            }
        }
    }

    Ok(())
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
    use crate::runtime::config_support::category_matcher::Category;
    use crate::runtime::config_support::test_helpers::write_temp_toml;
    use crate::runtime::config_support::test_load_missing_returns_empty;

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

    test_load_missing_returns_empty!(load);

    #[test]
    fn resolve_script_path_joins_overlay_root() {
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "scripts/test.ps1".to_string(),
            description: None,
        };
        let resolved = resolve_script_path(&entry, Path::new("/overlay"))
            .expect("relative script path should resolve");
        assert_eq!(resolved, PathBuf::from("/overlay/scripts/test.ps1"));
    }

    #[test]
    fn resolve_script_path_rejects_absolute_paths() {
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "/tmp/test.ps1".to_string(),
            description: None,
        };
        let err = resolve_script_path(&entry, Path::new("/overlay"))
            .expect_err("absolute script path should be rejected");
        assert!(err.to_string().contains("must be relative"));
    }

    #[test]
    fn resolve_script_path_rejects_parent_directory_traversal() {
        let entry = ScriptEntry {
            name: "test".to_string(),
            path: "scripts/../test.ps1".to_string(),
            description: None,
        };
        let err = resolve_script_path(&entry, Path::new("/overlay"))
            .expect_err("parent-directory traversal should be rejected");
        assert!(err.to_string().contains("must not contain '..'"));
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

    #[test]
    fn load_returns_error_on_unknown_field_in_entry() {
        let content = r#"
[base]
scripts = [
    { name = "Setup", path = "scripts/setup.sh", typo_field = "oops" },
]
"#;
        let (_dir, path) = write_temp_toml(content);
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "unknown field 'typo_field' in ScriptEntry should return an error"
        );
    }

    #[test]
    fn load_returns_error_on_unknown_section_field() {
        // Wrong field name in the section: "script" instead of "scripts".
        let content = "[base]\nscript = [{ name = \"Setup\", path = \"scripts/setup.sh\" }]\n";
        let (_dir, path) = write_temp_toml(content);
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "unknown section field 'script' should return an error"
        );
    }
}
