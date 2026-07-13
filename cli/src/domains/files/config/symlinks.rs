//! Symlink configuration loading.
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Component, Path, PathBuf};

use crate::runtime::config_support::Diagnostic;
use crate::runtime::config_support::config_section;

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
/// Returns an error when a glob is malformed, matches no entries, has
/// mismatched source/target wildcard counts, or expands to duplicate targets.
pub fn expand_glob_patterns(symlinks: &[Symlink], fallback: &Path) -> Result<Vec<Symlink>> {
    let mut expanded = Vec::new();
    for symlink in symlinks {
        expanded.extend(expand_one(symlink, fallback)?);
    }
    validate_unique_targets(&expanded)?;
    Ok(expanded)
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

#[derive(Debug)]
struct GlobMatch {
    relative_source: PathBuf,
    captures: Vec<String>,
}

fn expand_one(symlink: &Symlink, fallback: &Path) -> Result<Vec<Symlink>> {
    validate_supported_pattern("source", &symlink.source)?;
    if let Some(target) = &symlink.target {
        validate_supported_pattern("target", target)?;
    }

    let source_wildcards = wildcard_count(&symlink.source);
    let target_wildcards = symlink
        .target
        .as_ref()
        .map_or(0, |target| wildcard_count(target));
    if source_wildcards == 0 {
        if target_wildcards != 0 {
            bail!(
                "target pattern '{}' contains '*' but source '{}' is not a glob",
                symlink.target.as_deref().unwrap_or_default(),
                symlink.source
            );
        }
        return Ok(vec![symlink.clone()]);
    }
    validate_paths(symlink)?;
    if symlink.target.is_some() && source_wildcards != target_wildcards {
        bail!(
            "source pattern '{}' has {source_wildcards} wildcard(s), but target pattern '{}' has {target_wildcards}",
            symlink.source,
            symlink.target.as_deref().unwrap_or_default()
        );
    }

    let symlinks_dir = resolve_symlinks_dir(symlink, fallback);
    let matches = expand_segments(
        &symlinks_dir,
        &path_segments(&symlink.source),
        Path::new(""),
        &[],
    )
    .with_context(|| {
        format!(
            "expanding symlink glob '{}' under {}",
            symlink.source,
            symlinks_dir.display()
        )
    })?;
    if matches.is_empty() {
        bail!(
            "symlink glob '{}' matched no entries under {}",
            symlink.source,
            symlinks_dir.display()
        );
    }

    let mut expanded: Vec<Symlink> = matches
        .into_iter()
        .map(|glob_match| {
            let target = symlink
                .target
                .as_ref()
                .map(|target| apply_target_captures(target, &glob_match.captures))
                .transpose()?;
            Ok(Symlink {
                source: path_to_config_string(&glob_match.relative_source),
                target,
                origin: symlink.origin.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    expanded.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(expanded)
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

fn validate_supported_pattern(kind: &str, pattern: &str) -> Result<()> {
    for segment in path_segments(pattern) {
        if segment == "**" {
            bail!("{kind} pattern '{pattern}' uses unsupported recursive wildcard '**'");
        }
        if segment.contains('*') && segment != "*" {
            bail!(
                "{kind} pattern '{pattern}' uses unsupported wildcard segment '{segment}'; only a full path segment '*' is supported"
            );
        }
    }
    Ok(())
}

fn wildcard_count(pattern: &str) -> usize {
    path_segments(pattern)
        .into_iter()
        .filter(|segment| segment == "*")
        .count()
}

fn path_segments(path: &str) -> Vec<String> {
    path.split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Reports whether `path` is a real directory, without following symlinks.
///
/// Glob expansion recurses into directories via [`std::fs::read_dir`]. Using
/// [`Path::is_dir`] would follow a symlink-to-directory and recurse outside the
/// managed `symlinks/` tree. `symlink_metadata` inspects the entry itself, so a
/// symlink is never treated as a directory to descend into.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn expand_segments(
    base: &Path,
    remaining: &[String],
    relative: &Path,
    captures: &[String],
) -> Result<Vec<GlobMatch>> {
    let Some((segment, tail)) = remaining.split_first() else {
        return Ok(base
            .join(relative)
            .exists()
            .then(|| GlobMatch {
                relative_source: relative.to_path_buf(),
                captures: captures.to_vec(),
            })
            .into_iter()
            .collect());
    };

    if segment == "*" {
        let current = base.join(relative);
        if !is_real_dir(&current) {
            return Ok(Vec::new());
        }
        let mut entries: Vec<_> = std::fs::read_dir(&current)
            .with_context(|| format!("reading directory {}", current.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("reading directory entry in {}", current.display()))?;
        entries.sort_by_key(std::fs::DirEntry::path);

        let mut matches = Vec::new();
        for entry in entries {
            let capture = entry.file_name().to_string_lossy().into_owned();
            let mut next_captures = captures.to_vec();
            next_captures.push(capture.clone());
            matches.extend(expand_segments(
                base,
                tail,
                &relative.join(capture),
                &next_captures,
            )?);
        }
        return Ok(matches);
    }

    let next_relative = relative.join(segment);
    if !base.join(&next_relative).exists() {
        return Ok(Vec::new());
    }
    expand_segments(base, tail, &next_relative, captures)
}

fn apply_target_captures(target: &str, captures: &[String]) -> Result<String> {
    let mut captures = captures.iter();
    let mut segments = Vec::new();
    for segment in path_segments(target) {
        if segment == "*" {
            let Some(capture) = captures.next() else {
                bail!("target pattern '{target}' has more '*' wildcards than the source pattern");
            };
            segments.push(capture.clone());
        } else {
            segments.push(segment);
        }
    }
    if captures.next().is_some() {
        bail!("target pattern '{target}' has fewer '*' wildcards than the source pattern");
    }
    Ok(segments.join("/"))
}

fn path_to_config_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    let mut targets = HashMap::new();
    for symlink in symlinks {
        let target = target_key(symlink);
        match targets.entry(target) {
            Entry::Vacant(entry) => {
                entry.insert(symlink.source.clone());
            }
            Entry::Occupied(entry) => {
                bail!(
                    "symlink target collision for '{}': '{}' and '{}' both map to the same target",
                    entry.key(),
                    entry.get(),
                    symlink.source
                );
            }
        }
    }
    Ok(())
}

fn target_key(symlink: &Symlink) -> String {
    symlink
        .target
        .clone()
        .unwrap_or_else(|| format!(".{}", symlink.source))
        .replace('\\', "/")
}

/// Validate symlink entries and return any warnings.
#[must_use]
pub fn validate(symlinks: &[Symlink], root: &Path) -> Vec<Diagnostic> {
    use crate::runtime::config_support::validation::{CheckItem, Validator, check, check_error};

    Validator::new(SYMLINKS_TOML)
        .check_each(
            symlinks,
            |s| &s.source,
            |s| {
                let symlinks_dir = resolve_symlinks_dir(s, root);
                let source_path = symlinks_dir.join(&s.source);
                let target_checks: Vec<CheckItem> = s.target.as_ref().map_or_else(Vec::new, |t| {
                    vec![
                        check(
                            is_absolute_like(t),
                            "symlink.absolute-target",
                            "target path should be relative to $HOME directory",
                        ),
                        check_error(
                            has_parent_component(t),
                            "symlink.parent-in-target",
                            "target path must not contain '..' components",
                        ),
                    ]
                });
                let mut checks: Vec<CheckItem> = vec![
                    check(
                        !source_path.exists(),
                        "symlink.source-missing",
                        format!("source file does not exist: {}", source_path.display()),
                    ),
                    check(
                        is_absolute_like(&s.source),
                        "symlink.absolute-source",
                        "source path should be relative to symlinks/ directory",
                    ),
                    check_error(
                        has_parent_component(&s.source),
                        "symlink.parent-in-source",
                        "source path must not contain '..' components",
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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::runtime::config_support::category_matcher::Category;
    use crate::runtime::config_support::test_helpers::write_temp_toml;
    use crate::runtime::config_support::test_load_missing_returns_empty;

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
    fn expand_glob_patterns_rejects_duplicate_targets() {
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

        let result = expand_glob_patterns(&symlinks, temp_dir.path());
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
