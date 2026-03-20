//! Symlink configuration loading.
use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::ValidationWarning;
use super::config_section;

/// A symlink to create: source (in symlinks/) → target (in $HOME).
#[derive(Debug, Clone)]
pub struct Symlink {
    /// Relative path under symlinks/ directory.
    pub source: String,
    /// Explicit target path relative to `$HOME`; derived by convention when absent.
    pub target: Option<String>,
    /// Root of the repository that owns this symlink entry.
    /// Used to resolve `source` against `<origin>/symlinks/`.
    /// Set after loading — `None` until `set_origin` is called.
    pub origin: Option<PathBuf>,
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

config_section! {
    field: "symlinks",
    entry: SymlinkEntry,
    item: Symlink,
    map: |entry| match entry {
        SymlinkEntry::Simple(source) => Symlink {
            source,
            target: None,
            origin: None,
        },
        SymlinkEntry::WithTarget { source, target } => Symlink {
            source,
            target: Some(target),
            origin: None,
        },
    },
}

/// Set the `origin` field on every symlink entry to the given root.
pub fn set_origin(symlinks: &mut [Symlink], root: &Path) {
    for s in symlinks {
        s.origin = Some(root.to_path_buf());
    }
}

/// Resolve the symlinks directory for a single entry.
///
/// Returns `<origin>/symlinks/` when `origin` is set, otherwise falls back to
/// `<fallback>/symlinks/`.
#[must_use]
pub fn resolve_symlinks_dir(symlink: &Symlink, fallback: &Path) -> PathBuf {
    symlink
        .origin
        .as_deref()
        .unwrap_or(fallback)
        .join("symlinks")
}

/// Validate symlink entries and return any warnings.
#[must_use]
pub fn validate(symlinks: &[Symlink], root: &Path) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    Validator::new("symlinks.toml")
        .check_each(
            symlinks,
            |s| &s.source,
            |s| {
                let symlinks_dir = resolve_symlinks_dir(s, root);
                let source_path = symlinks_dir.join(&s.source);
                let target_checks: Vec<Option<String>> =
                    s.target.as_ref().map_or_else(Vec::new, |t| {
                        vec![
                            check(
                                Path::new(t).is_absolute() || t.starts_with('/'),
                                "target path should be relative to $HOME directory",
                            ),
                            check(
                                Path::new(t)
                                    .components()
                                    .any(|c| c == std::path::Component::ParentDir),
                                "target path must not contain '..' components",
                            ),
                        ]
                    });
                let mut checks = vec![
                    check(
                        !source_path.exists(),
                        format!("source file does not exist: {}", source_path.display()),
                    ),
                    check(
                        Path::new(&s.source).is_absolute() || s.source.starts_with('/'),
                        "source path should be relative to symlinks/ directory",
                    ),
                ];
                checks.extend(target_checks);
                checks
            },
        )
        .finish()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
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
            origin: None,
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
            origin: None,
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

    #[test]
    fn validate_detects_absolute_target() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "").unwrap();

        let symlinks = vec![Symlink {
            source: "bashrc".to_string(),
            target: Some("/etc/passwd".to_string()),
            origin: None,
        }];

        let warnings = validate(&symlinks, temp_dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0]
                .message
                .contains("should be relative to $HOME directory")
        );
    }

    #[test]
    fn validate_detects_target_path_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "").unwrap();

        let symlinks = vec![Symlink {
            source: "bashrc".to_string(),
            target: Some("../../etc/passwd".to_string()),
            origin: None,
        }];

        let warnings = validate(&symlinks, temp_dir.path());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("'..'"));
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nsymlinks = [\"bashrc\"");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nsymlinks = \"not-an-array\"\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "string instead of array should return error"
        );
    }
}
