//! Symlink configuration loading.
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

use crate::infra::config::Diagnostic;
use crate::infra::config::DiagnosticCode;
use crate::infra::config::StringOrTable;
use crate::infra::config::config_section;

/// Diagnostic code: `symlink.absolute-source`.
const SYMLINK_ABSOLUTE_SOURCE: DiagnosticCode = DiagnosticCode::new("symlink", "absolute-source");
/// Diagnostic code: `symlink.absolute-target`.
const SYMLINK_ABSOLUTE_TARGET: DiagnosticCode = DiagnosticCode::new("symlink", "absolute-target");
/// Diagnostic code: `symlink.parent-in-source`.
const SYMLINK_PARENT_IN_SOURCE: DiagnosticCode = DiagnosticCode::new("symlink", "parent-in-source");
/// Diagnostic code: `symlink.parent-in-target`.
const SYMLINK_PARENT_IN_TARGET: DiagnosticCode = DiagnosticCode::new("symlink", "parent-in-target");
/// Diagnostic code: `symlink.source-missing`.
const SYMLINK_SOURCE_MISSING: DiagnosticCode = DiagnosticCode::new("symlink", "source-missing");
/// Diagnostic code: `symlink.source-outside-root`.
const SYMLINK_SOURCE_OUTSIDE_ROOT: DiagnosticCode =
    DiagnosticCode::new("symlink", "source-outside-root");

mod glob_expansion;
mod target_capture;
mod target_validation;

/// A symlink to create: source (in symlinks/) → target (in $HOME).
#[derive(Debug, Clone)]
pub struct Symlink {
    /// Relative path under symlinks/ directory.
    pub source: String,
    /// Explicit target path relative to `$HOME`; derived by convention when absent.
    pub target: Option<String>,
    /// Root of the repository that owns this symlink entry.
    /// Used to resolve `source` against `<origin>/symlinks/`.
    ///
    /// `None` only transiently while a section is being loaded; [`set_origin`]
    /// runs as the post-load step in [`Config::load`](super::Config::load) and
    /// stamps every entry with its originating root (main or overlay), so the
    /// field is always `Some` by the time a [`Config`](super::Config) is
    /// returned. [`resolve_symlinks_dir`] falls back to a supplied root for the
    /// remaining `None` window.
    pub origin: Option<PathBuf>,
}

/// The explicit table form of a symlink entry: `{ source, target }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymlinkWithTarget {
    /// Relative path under the `symlinks/` directory.
    source: String,
    /// Explicit target path relative to `$HOME`.
    target: String,
}

/// A single entry in a symlinks section — either a plain source path or a
/// structured `{ source, target }` pair for an explicit target override.
type SymlinkEntry = StringOrTable<SymlinkWithTarget>;

config_section! {
    field: "symlinks",
    entry: SymlinkEntry,
    item: Symlink,
    map: |entry| match entry {
        StringOrTable::Bare(source) => Symlink {
            source,
            target: None,
            origin: None,
        },
        StringOrTable::Table(SymlinkWithTarget { source, target }) => Symlink {
            source,
            target: Some(target),
            origin: None,
        },
    },
}

/// Stamp the originating repository root onto every symlink entry.
///
/// This is the post-load provenance step invoked by
/// [`Config::load`](super::Config::load) once a section has been parsed: main
/// entries get the repo root and overlay entries get the overlay root, so
/// [`resolve_symlinks_dir`] can locate each entry's `symlinks/` directory.
pub(crate) fn set_origin(symlinks: &mut [Symlink], root: &Path) {
    for s in symlinks {
        s.origin = Some(root.to_path_buf());
    }
}

/// Expand source glob patterns into concrete symlink entries.
///
/// Glob support is intentionally small and deterministic: only a full path
/// segment of `*` is supported, and it captures exactly one source path
/// segment. If an explicit target contains `*`, each target wildcard is
/// replaced by the corresponding source capture in order.
///
/// # Errors
///
/// Returns an error when a glob is malformed, matches no entries, or has
/// mismatched source/target wildcard counts.
pub fn expand_glob_patterns(symlinks: &[Symlink], fallback: &Path) -> Result<Vec<Symlink>> {
    glob_expansion::expand_glob_patterns(symlinks, fallback)
}

/// Reject symlink entries that resolve to the same or overlapping targets.
///
/// # Errors
///
/// Returns an error identifying the conflicting sources and targets.
pub(crate) fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    target_validation::validate_unique_targets(symlinks)
}

/// Load symlink entries from every category without filtering.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub(crate) fn load_all(path: &Path) -> Result<Vec<Symlink>> {
    crate::infra::config::toml_loader::load_section_unfiltered::<Section>(path)
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

fn validate_relative_config_path(kind: &str, path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("{kind} path must not be empty");
    }
    if is_absolute_like(path) {
        bail!("{kind} path '{path}' must be relative");
    }
    if has_parent_component(path) {
        bail!("{kind} path '{path}' must not contain '..' components");
    }
    Ok(())
}

pub(crate) fn validate_paths(symlink: &Symlink) -> Result<()> {
    validate_relative_config_path("source", &symlink.source)?;
    if let Some(target) = &symlink.target {
        validate_relative_config_path("target", target)?;
    }
    Ok(())
}

fn source_containment_error(symlink: &Symlink, fallback: &Path) -> Option<String> {
    let symlinks_dir = resolve_symlinks_dir(symlink, fallback);
    let resolved_root = std::fs::canonicalize(&symlinks_dir).ok()?;
    let source = symlinks_dir.join(&symlink.source);
    let resolved_source = std::fs::canonicalize(&source).ok()?;
    (!resolved_source.starts_with(&resolved_root)).then(|| {
        format!(
            "source resolves outside its symlinks directory: {} -> {}",
            source.display(),
            resolved_source.display()
        )
    })
}

/// Reject a configured source that resolves outside its owning `symlinks/` tree.
///
/// Missing sources are handled separately by resource state discovery and do
/// not produce a containment error.
///
/// # Errors
///
/// Returns an error when the source canonicalizes outside the resolved
/// `symlinks/` directory.
pub(crate) fn validate_source_containment(symlink: &Symlink, fallback: &Path) -> Result<()> {
    if let Some(message) = source_containment_error(symlink, fallback) {
        bail!(message);
    }
    Ok(())
}

fn is_absolute_like(path: &str) -> bool {
    Path::new(path).is_absolute()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.as_bytes().get(1).is_some_and(|b| *b == b':')
}

fn has_parent_component(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
        || path.split(['/', '\\']).any(|segment| segment == "..")
}

pub(super) fn path_segments(path: &str) -> Vec<String> {
    path.split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Validate symlink entries and return any warnings.
#[must_use]
pub fn validate(symlinks: &[Symlink], root: &Path) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{CheckItem, Validator, check, check_error};

    Validator::new(SYMLINKS_TOML)
        .check_each(
            symlinks,
            |s| &s.source,
            |s| {
                let symlinks_dir = resolve_symlinks_dir(s, root);
                let source_path = symlinks_dir.join(&s.source);
                let containment_error = source_containment_error(s, root);
                let target_checks: Vec<CheckItem> = s.target.as_ref().map_or_else(Vec::new, |t| {
                    vec![
                        check(
                            is_absolute_like(t),
                            SYMLINK_ABSOLUTE_TARGET,
                            "target path should be relative to $HOME directory",
                        ),
                        check_error(
                            has_parent_component(t),
                            SYMLINK_PARENT_IN_TARGET,
                            "target path must not contain '..' components",
                        ),
                    ]
                });
                let mut checks: Vec<CheckItem> = vec![
                    check(
                        !source_path.exists(),
                        SYMLINK_SOURCE_MISSING,
                        format!("source file does not exist: {}", source_path.display()),
                    ),
                    check(
                        is_absolute_like(&s.source),
                        SYMLINK_ABSOLUTE_SOURCE,
                        "source path should be relative to symlinks/ directory",
                    ),
                    check_error(
                        has_parent_component(&s.source),
                        SYMLINK_PARENT_IN_SOURCE,
                        "source path must not contain '..' components",
                    ),
                    check_error(
                        containment_error.is_some(),
                        SYMLINK_SOURCE_OUTSIDE_ROOT,
                        containment_error.unwrap_or_else(|| {
                            "source must resolve inside its symlinks directory".to_string()
                        }),
                    ),
                ];
                checks.extend(target_checks);
                checks
            },
        )
        .finish()
}

/// TOML filename that backs this config section.
pub(crate) const SYMLINKS_TOML: &str = "symlinks.toml";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::{assert_load_rejects, write_temp_toml};
    use crate::infra::config::test_load_missing_returns_empty;

    #[test]
    fn unknown_key_in_symlink_table_is_rejected() {
        assert_load_rejects(
            load,
            r#"[base]
symlinks = [{ source = "bashrc", targett = ".bashrc" }]
"#,
            "targett",
        );
    }

    #[test]
    fn unknown_section_field_is_rejected() {
        assert_load_rejects(
            load,
            r#"[base]
symlink = ["bashrc"]
"#,
            "symlink",
        );
    }

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
    fn load_allows_canonical_source_with_distinct_targets() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
symlinks = ["config/shared"]

[windows]
symlinks = [
  { source = "config/shared", target = "AppData/Example/config" },
  { source = "config/shared", target = "Documents/Example/config" },
]
"#,
        );

        let symlinks = load(&path, &[Category::Base, Category::Windows]).unwrap();

        assert_eq!(symlinks.len(), 3);
        validate_unique_targets(&symlinks).unwrap();
    }

    #[test]
    fn load_all_ignores_category_filters() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
symlinks = ["bashrc"]

[desktop]
symlinks = ["config/i3"]

[windows]
symlinks = [{ source = "Documents/pwsh", target = "Documents/pwsh" }]
"#,
        );

        let symlinks = load_all(&path).unwrap();

        assert_eq!(symlinks.len(), 3);
        assert_eq!(symlinks[0].source, "bashrc");
        assert_eq!(symlinks[1].source, "config/i3");
        assert_eq!(symlinks[2].source, "Documents/pwsh");
    }

    test_load_missing_returns_empty!(load);

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
    fn validate_detects_source_path_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "../outside".to_string(),
            target: None,
            origin: None,
        }];

        let warnings = validate(&symlinks, temp_dir.path());
        assert!(
            warnings.iter().any(|w| w.message.contains("'..'")),
            "expected traversal warning, got: {warnings:?}"
        );
    }

    #[test]
    fn expand_glob_patterns_rejects_glob_path_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "../*".to_string(),
            target: Some("../../outside".to_string()),
            origin: None,
        }];

        let err = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap_err();
        assert!(err.to_string().contains("must not contain '..'"));
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

    #[cfg(unix)]
    #[test]
    fn validate_detects_source_symlink_outside_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        let outside = outside_dir.path().join("outside");
        std::fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, symlinks_dir.join("escaped")).unwrap();
        let symlink = Symlink {
            source: "escaped".to_string(),
            target: None,
            origin: None,
        };

        let diagnostics = validate(std::slice::from_ref(&symlink), temp_dir.path());

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.to_string(),
            "symlink.source-outside-root"
        );
        assert!(diagnostics[0].message.contains("outside"));
        assert!(validate_source_containment(&symlink, temp_dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validate_allows_source_symlink_inside_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        let canonical = symlinks_dir.join("canonical");
        std::fs::write(&canonical, "inside").unwrap();
        std::os::unix::fs::symlink(&canonical, symlinks_dir.join("alias")).unwrap();
        let symlink = Symlink {
            source: "alias".to_string(),
            target: None,
            origin: None,
        };

        assert!(validate(&[symlink], temp_dir.path()).is_empty());
    }

    #[test]
    fn validate_unique_targets_rejects_explicit_and_computed_collision() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "").unwrap();
        std::fs::write(symlinks_dir.join("other"), "").unwrap();
        let symlinks = vec![
            Symlink {
                source: "bashrc".to_string(),
                target: None,
                origin: None,
            },
            Symlink {
                source: "other".to_string(),
                target: Some(".bashrc".to_string()),
                origin: None,
            },
        ];

        let error = validate_unique_targets(&symlinks).unwrap_err();
        assert!(error.to_string().contains("collision"));
    }

    #[test]
    fn validate_unique_targets_rejects_ancestor_overlap_in_either_order() {
        let parent = Symlink {
            source: "config/example".to_string(),
            target: None,
            origin: None,
        };
        let child = Symlink {
            source: "example-file".to_string(),
            target: Some(".config/example/file".to_string()),
            origin: None,
        };

        for symlinks in [vec![parent.clone(), child.clone()], vec![child, parent]] {
            let error = validate_unique_targets(&symlinks).unwrap_err();
            assert!(error.to_string().contains("overlap"));
            assert!(error.to_string().contains(".config/example"));
            assert!(error.to_string().contains(".config/example/file"));
        }
    }

    #[test]
    fn validate_unique_targets_allows_shared_textual_prefix() {
        let symlinks = vec![
            Symlink {
                source: "one".to_string(),
                target: Some(".config/example".to_string()),
                origin: None,
            },
            Symlink {
                source: "two".to_string(),
                target: Some(".config/example-extra/file".to_string()),
                origin: None,
            },
        ];

        validate_unique_targets(&symlinks).unwrap();
    }

    #[test]
    fn expand_glob_patterns_expands_skill_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let skills_dir = temp_dir.path().join("symlinks").join("skills");
        std::fs::create_dir_all(skills_dir.join("alpha")).unwrap();
        std::fs::create_dir_all(skills_dir.join("bravo")).unwrap();

        let symlinks = vec![Symlink {
            source: "skills/*".to_string(),
            target: Some(".copilot/skills/*".to_string()),
            origin: None,
        }];

        let expanded = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap();
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].source, "skills/alpha");
        assert_eq!(expanded[0].target.as_deref(), Some(".copilot/skills/alpha"));
        assert_eq!(expanded[1].source, "skills/bravo");
        assert_eq!(expanded[1].target.as_deref(), Some(".copilot/skills/bravo"));
    }

    #[test]
    fn expand_glob_patterns_preserves_origin() {
        let temp_dir = tempfile::tempdir().unwrap();
        let origin = temp_dir.path().join("overlay");
        std::fs::create_dir_all(origin.join("symlinks").join("skills").join("alpha")).unwrap();

        let symlinks = vec![Symlink {
            source: "skills/*".to_string(),
            target: Some(".copilot/skills/*".to_string()),
            origin: Some(origin.clone()),
        }];

        let expanded = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].origin.as_deref(), Some(origin.as_path()));
    }

    #[test]
    fn expand_glob_patterns_rejects_mismatched_target_wildcards() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "skills/*".to_string(),
            target: Some(".copilot/skills".to_string()),
            origin: None,
        }];

        let result = expand_glob_patterns(&symlinks, temp_dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("wildcard"));
    }

    #[test]
    fn expand_glob_patterns_rejects_recursive_wildcard() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![Symlink {
            source: "skills/**".to_string(),
            target: Some(".copilot/skills/*".to_string()),
            origin: None,
        }];

        let result = expand_glob_patterns(&symlinks, temp_dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("**"));
    }

    #[test]
    fn validate_unique_targets_rejects_duplicate_explicit_targets() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = temp_dir.path().join("symlinks");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("one"), "").unwrap();
        std::fs::write(symlinks_dir.join("two"), "").unwrap();
        let symlinks = vec![
            Symlink {
                source: "one".to_string(),
                target: Some(".same".to_string()),
                origin: None,
            },
            Symlink {
                source: "two".to_string(),
                target: Some(".same".to_string()),
                origin: None,
            },
        ];

        let result = validate_unique_targets(&symlinks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("collision"));
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
