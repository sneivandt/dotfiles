//! Symlink configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

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

impl toml_loader::ConfigSection for SymlinkSection {
    type Entry = SymlinkEntry;
    type Item = Symlink;
    const MATCH_MODE: MatchMode = MatchMode::All;

    fn extract(self) -> Vec<SymlinkEntry> {
        self.symlinks
    }

    fn map(entry: SymlinkEntry) -> Symlink {
        match entry {
            SymlinkEntry::Simple(source) => Symlink {
                source,
                target: None,
            },
            SymlinkEntry::WithTarget { source, target } => Symlink {
                source,
                target: Some(target),
            },
        }
    }
}

/// Load symlinks from symlinks.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<Symlink>> {
    toml_loader::load_section::<SymlinkSection>(path, active_categories)
}

/// Validate symlink entries and return any warnings.
#[must_use]
pub fn validate(symlinks: &[Symlink], root: &Path) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    let symlinks_dir = root.join("symlinks");
    Validator::new("symlinks.toml")
        .check_each(
            symlinks,
            |s| &s.source,
            |s| {
                let source_path = symlinks_dir.join(&s.source);
                vec![
                    check(
                        !source_path.exists(),
                        format!("source file does not exist: {}", source_path.display()),
                    ),
                    check(
                        Path::new(&s.source).is_absolute() || s.source.starts_with('/'),
                        "source path should be relative to symlinks/ directory",
                    ),
                ]
            },
        )
        .finish()
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

    #[test]
    fn validate_detects_missing_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "nonexistent.txt".to_string(),
            target: None,
        }];

        let warnings = validate(&symlinks, temp_dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("does not exist"));
    }

    #[test]
    fn validate_detects_absolute_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "/absolute/path".to_string(),
            target: None,
        }];

        let warnings = validate(&symlinks, temp_dir.path());
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("should be relative"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("does not exist"))
        );
    }
}
