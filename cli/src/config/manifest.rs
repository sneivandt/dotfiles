//! Sparse-checkout manifest configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::helpers::category_matcher::Category;
use super::helpers::toml_loader;

/// Sparse checkout manifest — files to exclude by category.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Files that should be excluded (in excluded categories).
    pub excluded_files: Vec<String>,
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
    let config: HashMap<String, ManifestSection> = toml_loader::load_config(path)?;

    let items: Vec<(String, Vec<String>)> = config.into_iter().map(|(k, v)| (k, v.paths)).collect();

    let excluded_files = toml_loader::filter_by_categories(items, excluded_categories);

    Ok(Manifest { excluded_files })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_toml;

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
