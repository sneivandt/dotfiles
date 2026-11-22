//! Script entry configuration for overlay custom tasks.
//!
//! Scripts are defined in `scripts.toml` inside an overlay repository.
//! Each script entry specifies a name, a path to a script file relative to
//! the overlay root, and an optional description.
use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::infra::config::Diagnostic;
use crate::infra::config::config_section;

crate::infra::config::diagnostic_codes! {
    "scripts";
    SCRIPTS_DUPLICATE_SELECTOR => "duplicate-selector",
    SCRIPTS_INVALID_PATH => "invalid-path",
    SCRIPTS_EMPTY_NAME => "empty-name",
}

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
    /// Use `--check-remove`, whose success means any managed state remains.
    #[serde(default)]
    pub check_remove: bool,
}

config_section!(field: "scripts", ty: ScriptEntry);

/// TOML filename that backs this config section.
pub(crate) const SCRIPTS_TOML: &str = "scripts.toml";

/// Validate overlay script entries.
#[must_use]
pub fn validate(entries: &[ScriptEntry]) -> Vec<Diagnostic> {
    use crate::infra::config::Severity;
    use crate::infra::config::validation::{Validator, check_error};

    let mut selectors = HashSet::new();
    let mut duplicates = HashSet::new();
    for entry in entries {
        let selector = overlay_script_selector(&entry.name);
        if !selectors.insert(selector.clone()) {
            duplicates.insert(selector);
        }
    }

    Validator::new(SCRIPTS_TOML)
        .check_each(
            entries,
            |entry| &entry.name,
            |entry| {
                [
                    check_error(
                        entry.name.trim().is_empty(),
                        SCRIPTS_EMPTY_NAME,
                        "script name must not be empty",
                    ),
                    validate_relative_script_path(entry)
                        .err()
                        .map(|error| (SCRIPTS_INVALID_PATH, Severity::Error, error.to_string())),
                    check_error(
                        duplicates.contains(&overlay_script_selector(&entry.name)),
                        SCRIPTS_DUPLICATE_SELECTOR,
                        "script name normalizes to the same CLI selector as another entry",
                    ),
                ]
            },
        )
        .finish()
}

/// Build the stable CLI selector for an overlay script name.
#[must_use]
pub(crate) fn overlay_script_selector(name: &str) -> String {
    let suffix = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-");
    format!("script-{suffix}")
}

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
    if entry.path.starts_with('/')
        || entry.path.starts_with('\\')
        || entry
            .path
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':')
    {
        bail!("script path for '{}' must be relative", entry.name);
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
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::{rejects_toml, write_temp_toml};
    use crate::infra::config::test_load_missing_returns_empty;

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

    fn script(name: &str, path: &str) -> ScriptEntry {
        ScriptEntry {
            name: name.into(),
            path: path.into(),
            description: None,
            check_remove: false,
        }
    }

    #[test]
    fn validate_rejects_empty_names_invalid_paths_and_duplicate_selectors() {
        let entries = [
            script(" ", ""),
            script("Duplicate", "../escape.sh"),
            script("Duplicate", "../escape.sh"),
        ];

        let diagnostics = validate(&entries);
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.to_string())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"scripts.empty-name".to_string()));
        assert!(codes.contains(&"scripts.invalid-path".to_string()));
        assert!(codes.contains(&"scripts.duplicate-selector".to_string()));
    }

    #[test]
    fn validate_rejects_distinct_names_with_the_same_selector() {
        let entries = [
            script("Set up DB", "scripts/setup-db.sh"),
            script("Set-up DB", "scripts/setup-db-alt.sh"),
        ];

        let diagnostics = validate(&entries);

        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code.to_string() == "scripts.duplicate-selector"),
            "unexpected diagnostics: {diagnostics:?}"
        );
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn resolve_script_path_joins_overlay_root() {
        let entry = script("test", "scripts/test.ps1");
        let resolved = resolve_script_path(&entry, Path::new("/overlay"))
            .expect("relative script path should resolve");
        assert_eq!(resolved, PathBuf::from("/overlay/scripts/test.ps1"));
    }

    #[test]
    fn resolve_script_path_rejects_unsafe_paths_on_every_platform() {
        for (path, fragment) in [
            ("/tmp/test.ps1", "must be relative"),
            (r"C:\tmp\test.ps1", "must be relative"),
            ("scripts/../test.ps1", "must not contain '..'"),
        ] {
            let err =
                resolve_script_path(&script("test", path), Path::new("/overlay")).unwrap_err();
            assert!(err.to_string().contains(fragment), "{path}: {err}");
        }
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

    rejects_toml! {
        "test.toml", |path| load(path, &[Category::Base]);
        load_returns_error_on_unknown_field_in_entry: r#"
[base]
scripts = [
    { name = "Setup", path = "scripts/setup.sh", typo_field = "oops" },
]
"#;
        load_returns_error_on_unknown_section_field:
            "[base]\nscript = [{ name = \"Setup\", path = \"scripts/setup.sh\" }]\n";
    }
}
