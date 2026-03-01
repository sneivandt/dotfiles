//! Symlink configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

/// A symlink to create: source (in symlinks/) → target (in $HOME).
#[derive(Debug, Clone)]
pub struct Symlink {
    /// Relative path under symlinks/ directory.
    pub source: String,
    /// Explicit target path relative to `$HOME`; derived by convention when absent.
    pub target: Option<String>,
}

/// A single entry in a symlinks section — either a plain source path or a
/// structured `{ source, target }` pair for an explicit target override.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SymlinkEntry {
    /// Plain string: `"bashrc"` — target is derived by convention.
    Simple(String),
    /// Structured: `{ source = "foo", target = ".bar" }` — explicit target.
    WithTarget { source: String, target: String },
}

/// TOML section containing symlinks.
#[derive(Debug, Deserialize)]
struct SymlinkSection {
    symlinks: Vec<SymlinkEntry>,
}

/// Load symlinks from symlinks.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<Symlink>> {
    let items = toml_loader::load_section_items(path, |s: SymlinkSection| s.symlinks)?;

    let entries: Vec<SymlinkEntry> =
        toml_loader::filter_by_categories(items, active_categories, MatchMode::All);

    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            SymlinkEntry::Simple(source) => Symlink {
                source,
                target: None,
            },
            SymlinkEntry::WithTarget { source, target } => Symlink {
                source,
                target: Some(target),
            },
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_base_symlinks() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
symlinks = ["bashrc", "config/git/config"]

[desktop]
symlinks = ["config/i3"]
"#,
        );
        let symlinks: Vec<Symlink> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(symlinks.len(), 2);
        assert_eq!(symlinks[0].source, "bashrc");
        assert_eq!(symlinks[1].source, "config/git/config");
    }

    #[test]
    fn load_multi_category() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
symlinks = ["bashrc"]

["arch-desktop"]
symlinks = ["config/i3"]
"#,
        );
        let symlinks: Vec<Symlink> =
            load(&path, &[Category::Base, Category::Arch, Category::Desktop]).unwrap();
        assert_eq!(symlinks.len(), 2);
    }

    #[test]
    fn load_explicit_target_override() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
symlinks = [
  "bashrc",
  { source = "config/something", target = ".custom-name" },
]
"#,
        );
        let symlinks: Vec<Symlink> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(symlinks.len(), 2);
        assert_eq!(symlinks[0].source, "bashrc");
        assert!(symlinks[0].target.is_none());
        assert_eq!(symlinks[1].source, "config/something");
        assert_eq!(symlinks[1].target.as_deref(), Some(".custom-name"));
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
