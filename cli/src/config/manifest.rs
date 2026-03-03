//! Sparse-checkout manifest derived from symlinks.toml.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

/// Sparse checkout manifest — files to exclude by category.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Files that should be excluded (in excluded categories).
    pub excluded_files: Vec<String>,
}

/// A single entry in a symlinks section — either a plain source path or a
/// structured `{ source, target }` pair.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SymlinkEntry {
    Simple(String),
    WithTarget {
        source: String,
        #[allow(dead_code)]
        target: String,
    },
}

impl SymlinkEntry {
    fn into_source(self) -> String {
        match self {
            Self::Simple(s) => s,
            Self::WithTarget { source, .. } => source,
        }
    }
}

/// TOML section containing symlinks.
#[derive(Debug, Deserialize)]
struct SymlinkSection {
    symlinks: Vec<SymlinkEntry>,
}

/// Derive excluded files from symlinks.toml using OR exclusion logic.
///
/// A section is excluded if ANY of its category tags match the excluded set.
/// This is the opposite of the symlink loader which uses AND inclusion logic.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, excluded_categories: &[Category]) -> Result<Manifest> {
    let items = toml_loader::load_section_items(path, |s: SymlinkSection| s.symlinks)?;

    let entries: Vec<SymlinkEntry> =
        toml_loader::filter_by_categories(items, excluded_categories, MatchMode::Any);

    let excluded_files: Vec<String> = entries.into_iter().map(SymlinkEntry::into_source).collect();

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
symlinks = ["file1"]

[arch]
symlinks = ["file2"]

[windows]
symlinks = ["file3"]

[arch-desktop]
symlinks = ["file4"]
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
symlinks = ["file1"]
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
symlinks = ["file1"]

[arch]
symlinks = ["file2"]
"#,
        );
        let manifest = load(&path, &[Category::Windows]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "no categories matched — nothing should be excluded"
        );
    }

    #[test]
    fn extracts_source_from_explicit_target() {
        let (_dir, path) = write_temp_toml(
            r#"[windows]
symlinks = [
  "simple_file",
  { source = "AppData/path", target = "AppData/path" },
]
"#,
        );
        let manifest = load(&path, &[Category::Windows]).unwrap();
        assert_eq!(manifest.excluded_files.len(), 2);
        assert!(manifest.excluded_files.contains(&"simple_file".to_string()));
        assert!(
            manifest
                .excluded_files
                .contains(&"AppData/path".to_string())
        );
    }
}
