//! Sparse-checkout manifest configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::infra::config::category_matcher::Category;
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
struct ManifestSection {
    paths: Vec<String>,
}

/// Load manifest from manifest.toml using AND exclusion logic.
///
/// A file section is excluded only if ALL of its category tags match the
/// excluded set — the same logic used by all other config files.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, excluded_categories: &[Category]) -> Result<Manifest> {
    let config: BTreeMap<String, ManifestSection> = toml_loader::load_optional_config(path)?;

    let items: Vec<(String, Vec<String>)> = config.into_iter().map(|(k, v)| (k, v.paths)).collect();

    let excluded_files = toml_loader::filter_by_categories(items, excluded_categories);

    Ok(Manifest { excluded_files })
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
    use crate::infra::config::test_helpers::write_temp_toml;

    #[test]
    fn and_exclusion_logic() {
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
        // Excluding 'windows' excludes file3 (single-category match),
        // but NOT file4 — [arch-desktop] requires BOTH arch AND desktop to be excluded.
        let manifest = load(&path, &[Category::Windows]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file3"]);
    }

    #[test]
    fn and_logic_multi_category_both_excluded() {
        let (_dir, path) = write_temp_toml(
            r#"[arch-desktop]
paths = ["file1"]
"#,
        );
        // Excluding both 'arch' and 'desktop' excludes the section (AND logic)
        let manifest = load(&path, &[Category::Arch, Category::Desktop]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file1"]);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = load(&dir.path().join("nope.toml"), &[Category::Windows]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "missing file should produce empty manifest"
        );
    }

    #[test]
    fn excludes_nothing_when_no_categories_match() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
paths = ["file1"]

[arch]
paths = ["file2"]
"#,
        );
        let manifest = load(&path, &[Category::Windows]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "no categories matched — nothing should be excluded"
        );
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
