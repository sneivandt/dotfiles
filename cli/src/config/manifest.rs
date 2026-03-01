//! Sparse-checkout manifest configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

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

/// Load manifest from manifest.toml using OR exclusion logic.
///
/// A file section is excluded if ANY of its category tags match the excluded set.
/// This is the opposite of other config files which use AND inclusion logic.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, excluded_categories: &[Category]) -> Result<Manifest> {
    let config: HashMap<String, ManifestSection> = toml_loader::load_config(path)?;

    let items: Vec<(String, Vec<String>)> = config.into_iter().map(|(k, v)| (k, v.paths)).collect();

    let excluded_files =
        toml_loader::filter_by_categories(items, excluded_categories, MatchMode::Any);

    Ok(Manifest { excluded_files })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_toml;

    #[test]
    fn or_exclusion_logic() {
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
        // Excluding 'windows' should exclude only file3
        let manifest = load(&path, &[Category::Windows]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file3"]);
    }

    #[test]
    fn or_logic_multi_category() {
        let (_dir, path) = write_temp_toml(
            r#"[arch-desktop]
paths = ["file1"]
"#,
        );
        // Excluding just 'arch' should still exclude the section (OR logic)
        let manifest = load(&path, &[Category::Arch]).unwrap();
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
}
