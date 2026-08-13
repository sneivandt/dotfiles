//! Sparse-checkout manifest configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::infra::config::category_matcher::{self, Category};
use crate::infra::config::toml_loader;

/// Sparse checkout manifest — files to exclude by category.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Files that should be excluded (in excluded categories).
    pub excluded_files: Vec<String>,
}

impl Manifest {
    /// Return whether a symlink source is covered by an excluded manifest path.
    #[must_use]
    pub fn excludes_source(&self, source: &str) -> bool {
        let source = source.replace('\\', "/");
        self.excluded_files.iter().any(|excluded| {
            let excluded = excluded.replace('\\', "/");
            excluded.strip_suffix('/').map_or_else(
                || source == excluded,
                |directory| source == directory || source.starts_with(&excluded),
            )
        })
    }
}

/// TOML section containing excluded paths.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSection {
    paths: Vec<String>,
}

/// Load the sparse-checkout manifest for the active category set.
///
/// A path is retained when at least one section that declares it is active.
/// It is excluded only when every declaring section is inactive. This supports
/// paths shared by multiple category combinations without letting one inactive
/// owner hide a path needed by another active owner.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(config_path: &Path, active_categories: &[Category]) -> Result<Manifest> {
    let config: BTreeMap<String, ManifestSection> = toml_loader::load_optional_config(config_path)?;

    let mut path_activity = BTreeMap::<String, bool>::new();
    for (section, manifest) in config {
        let section_active = category_matcher::matches(
            &category_matcher::parse_section_key(&section),
            active_categories,
        );
        for manifest_path in manifest.paths {
            path_activity
                .entry(manifest_path)
                .and_modify(|active| *active |= section_active)
                .or_insert(section_active);
        }
    }
    let excluded_files = path_activity
        .into_iter()
        .filter_map(|(manifest_path, active)| (!active).then_some(manifest_path))
        .collect();

    Ok(Manifest { excluded_files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::test_helpers::{assert_load_rejects, write_temp_toml};

    #[test]
    fn unknown_key_in_manifest_section_is_rejected() {
        assert_load_rejects(
            load,
            r#"[base]
path = ["file1"]
"#,
            "path",
        );
    }

    #[test]
    fn inactive_compound_section_is_excluded() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
paths = ["file1"]

[arch]
paths = ["file2"]

[windows]
paths = ["file3"]

[arch-desktop]
paths = ["file4"]
"#,
        );
        let manifest = load(&path, &[Category::Base, Category::Arch]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file3", "file4"]);
    }

    #[test]
    fn active_compound_section_is_retained() {
        let (_dir, path) = write_temp_toml(
            r#"[arch-desktop]
paths = ["file1"]
"#,
        );
        let manifest = load(&path, &[Category::Base, Category::Arch, Category::Desktop]).unwrap();
        assert!(manifest.excluded_files.is_empty());
    }

    #[test]
    fn shared_path_is_retained_when_any_owner_is_active() {
        let (_dir, path) = write_temp_toml(
            r#"[desktop]
paths = ["shared"]

[windows-desktop]
paths = ["shared"]
"#,
        );
        let manifest = load(&path, &[Category::Base, Category::Desktop]).unwrap();
        assert!(manifest.excluded_files.is_empty());
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = load(&dir.path().join("nope.toml"), &[Category::Base]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "missing file should produce empty manifest"
        );
    }

    #[test]
    fn excludes_sections_without_active_categories() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
paths = ["file1"]

[arch]
paths = ["file2"]
"#,
        );
        let manifest = load(&path, &[Category::Base, Category::Windows]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file2"]);
    }

    #[test]
    fn excludes_source_matches_files_and_directory_contents() {
        let manifest = Manifest {
            excluded_files: vec![
                "config/i3/".to_string(),
                "config/Code/User/settings.json".to_string(),
            ],
        };

        assert!(manifest.excludes_source("config/i3"));
        assert!(manifest.excludes_source("config/i3/config"));
        assert!(manifest.excludes_source("config/i3/*"));
        assert!(manifest.excludes_source("config/Code/User/settings.json"));
        assert!(!manifest.excludes_source("config/i3status/config"));
        assert!(!manifest.excludes_source("config/Code/User/keybindings.json"));
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\npaths = [\"file1\"");
        let result = load(&path, &[Category::Windows]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\npaths = \"not-an-array\"\n");
        let result = load(&path, &[Category::Windows]);
        assert!(
            result.is_err(),
            "string instead of array should return error"
        );
    }
}
