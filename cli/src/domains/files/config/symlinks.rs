//! Symlink configuration loading.
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

use crate::infra::config::Diagnostic;
use crate::infra::config::StringOrTable;
use crate::infra::config::config_section;

crate::infra::config::diagnostic_codes! {
    "symlink";
    SYMLINK_ABSOLUTE_SOURCE => "absolute-source",
    SYMLINK_ABSOLUTE_TARGET => "absolute-target",
    SYMLINK_PARENT_IN_SOURCE => "parent-in-source",
    SYMLINK_PARENT_IN_TARGET => "parent-in-target",
    SYMLINK_SOURCE_MISSING => "source-missing",
    SYMLINK_SOURCE_INSPECTION => "source-inspection",
    SYMLINK_SOURCE_OUTSIDE_ROOT => "source-outside-root",
    SYMLINK_ROOT_SOURCE => "root-source",
    SYMLINK_ROOT_TARGET => "root-target",
    SYMLINK_CONFIG_HOME => "unsupported-config-home",
}

mod glob_expansion;
mod target_capture;
mod target_validation;

pub(crate) use crate::infra::fs::path_identity;
pub(crate) use target_validation::target_identity;

/// A symlink to create: source (in symlinks/) → target (in $HOME).
#[derive(Debug, Clone)]
pub struct Symlink {
    /// Relative path under symlinks/ directory.
    pub source: String,
    /// Explicit target path relative to `$HOME`; derived by convention when absent.
    pub target: Option<String>,
    /// Allow existing parent links to resolve the target outside the home directory.
    pub allow_external_target: bool,
    /// Root of the repository that owns this symlink entry.
    /// Used to resolve `source` against `<origin>/symlinks/`.
    ///
    /// `None` only transiently while a section is being loaded; [`set_origin`]
    /// runs as the post-load step in [`Config::load`](crate::app::config::Config::load) and
    /// stamps every entry with its originating root (main or overlay), so the
    /// field is always `Some` by the time a [`Config`](crate::app::config::Config) is
    /// returned. [`resolve_symlinks_dir`] falls back to a supplied root for the
    /// remaining `None` window.
    pub origin: Option<PathBuf>,
}

pub(crate) fn config_home_diagnostic(
    symlinks: &[Symlink],
    home: &Path,
    env: &dyn crate::infra::env::Env,
) -> Option<Diagnostic> {
    let configured = env
        .var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())?;
    let expected = home.join(".config");
    let config_target = path_identity(Path::new(".config"));
    if Path::new(&configured) == expected
        || !symlinks
            .iter()
            .any(|link| target_identity(link).starts_with(&config_target))
    {
        return None;
    }
    Some(Diagnostic::error(
        SYMLINKS_TOML,
        "XDG_CONFIG_HOME",
        SYMLINK_CONFIG_HOME,
        format!(
            "managed application settings require XDG_CONFIG_HOME={} (or unset); custom configuration layouts are not supported",
            expected.display()
        ),
    ))
}

/// The explicit table form of a symlink entry: `{ source, target }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymlinkWithTarget {
    /// Relative path under the `symlinks/` directory.
    source: String,
    /// Explicit target path relative to `$HOME`.
    target: String,
    #[serde(default)]
    allow_external_target: bool,
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
            allow_external_target: false,
            origin: None,
        },
        StringOrTable::Table(SymlinkWithTarget { source, target, allow_external_target }) => Symlink {
            source,
            target: Some(target),
            allow_external_target,
            origin: None,
        },
    },
}

pub(crate) fn declaring_section(
    entry: &Symlink,
    root: &Path,
    categories: &[crate::infra::config::category_matcher::Category],
) -> Result<Option<String>> {
    use crate::infra::config::toml_loader::{ConfigDocument, read_config};
    let origin = entry.origin.as_deref().unwrap_or(root);
    let path = origin.join("conf").join(SYMLINKS_TOML);
    let content = read_config(&path, false)?;
    let sections = decode(&ConfigDocument::parse(&path, &content)?)?;
    for (name, mut items) in sections {
        let selected = crate::infra::config::toml_loader::filter_by_categories(
            vec![(name.clone(), vec![()])],
            categories,
        );
        if selected.is_empty() {
            continue;
        }
        set_origin(&mut items, origin);
        if expand_glob_patterns(&items, root)?.iter().any(|item| {
            item.source == entry.source && target_identity(item) == target_identity(entry)
        }) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// Stamp the originating repository root onto every symlink entry.
///
/// This is the post-load provenance step invoked by
/// [`Config::load`](crate::app::config::Config::load) once a section has been parsed: main
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

/// Expand optional source glob patterns, returning empty matches for absent
/// source trees while preserving pattern and filesystem inspection errors.
pub(crate) fn expand_optional_glob_patterns(
    symlinks: &[Symlink],
    fallback: &Path,
) -> Result<Vec<Symlink>> {
    glob_expansion::expand_optional_glob_patterns(symlinks, fallback)
}

/// Reject symlink entries that resolve to the same or overlapping targets.
///
/// # Errors
///
/// Returns an error identifying the conflicting sources and targets.
pub(crate) fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    target_validation::validate_unique_targets(symlinks)
}

pub(crate) use target_validation::validate_target_paths;

/// Load symlink entries from every category without filtering.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
#[cfg(test)]
fn load_all(path: &Path) -> Result<Vec<Symlink>> {
    Ok(
        crate::infra::config::toml_loader::with_optional_document(path, decode)?
            .into_iter()
            .flat_map(|(_, items)| items)
            .collect(),
    )
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
    if points_to_root(path) {
        bail!("{kind} path '{path}' must name a file or directory below its root");
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

pub(crate) enum InspectedSource {
    Available(PathBuf),
    Missing(String),
    Outside(String),
}

pub(crate) fn inspect_source(source: &Path, directory: Option<&Path>) -> Result<InspectedSource> {
    let resolved = match dunce::canonicalize(source) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InspectedSource::Missing(format!(
                "source does not exist: {}",
                source.display()
            )));
        }
        result => result.with_context(|| format!("inspect source: {}", source.display()))?,
    };
    if let Some(directory) = directory
        && !resolved.starts_with(crate::infra::fs::canonicalize(directory)?)
    {
        return Ok(InspectedSource::Outside(format!(
            "source resolves outside its symlinks directory: {} -> {}",
            source.display(),
            resolved.display()
        )));
    }
    Ok(InspectedSource::Available(resolved))
}

fn is_absolute_like(path: &str) -> bool {
    path.starts_with(['/', '\\']) || path.as_bytes().get(1) == Some(&b':')
}

fn has_parent_component(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
        || path.split(['/', '\\']).any(|segment| segment == "..")
}

fn points_to_root(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|component| matches!(component, Component::CurDir))
}

/// Compute the home-relative default before comparing or creating targets.
pub(crate) fn default_target(source: &str) -> String {
    let normalized = Path::new(source)
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    format!(".{normalized}")
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
                let source_check = match inspect_source(&source_path, Some(&symlinks_dir)) {
                    Ok(InspectedSource::Available(_)) => None,
                    Ok(InspectedSource::Missing(reason)) => {
                        check(true, SYMLINK_SOURCE_MISSING, reason)
                    }
                    Ok(InspectedSource::Outside(reason)) => {
                        check_error(true, SYMLINK_SOURCE_OUTSIDE_ROOT, reason)
                    }
                    Err(error) => {
                        check_error(true, SYMLINK_SOURCE_INSPECTION, format!("{error:#}"))
                    }
                };
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
                        check_error(
                            points_to_root(t),
                            SYMLINK_ROOT_TARGET,
                            "target must name a file or directory below $HOME",
                        ),
                    ]
                });
                let mut checks: Vec<CheckItem> = vec![
                    source_check,
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
                        points_to_root(&s.source),
                        SYMLINK_ROOT_SOURCE,
                        "source must name a file or directory below symlinks/",
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
    use crate::infra::config::test_helpers::{
        assert_mentions, assert_messages, rejects_toml, write_temp_toml,
    };
    use crate::infra::config::test_load_missing_returns_empty;
    use crate::test_helpers::symlink;

    rejects_toml! {
        "test.toml", |path| load(path, &[Category::Base]);
        unknown_key_in_symlink_table_is_rejected: r#"[base]
symlinks = [{ source = "bashrc", targett = ".bashrc" }]
"# => "targett";
        unknown_section_field_is_rejected: r#"[base]
symlink = ["bashrc"]
"# => "symlink";
        load_returns_error_on_malformed_toml: "[base\nsymlinks = [\"bashrc\"";
        load_returns_error_on_type_mismatch: "[base]\nsymlinks = \"not-an-array\"\n";
    }

    #[test]
    fn load_base_symlinks() {
        for abs in ["/abs/path", "\\win\\path", "C:\\drive\\path", "D:rel"] {
            assert!(is_absolute_like(abs), "expected absolute-like: {abs}");
        }
        assert!(!is_absolute_like("relative/path"));
        assert!(!is_absolute_like("a"));

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
    fn validate_reports_missing_absolute_and_traversing_sources() {
        for (source, count, messages) in [
            ("nonexistent.txt", Some(1), &["does not exist"][..]),
            (
                "/absolute/path",
                Some(2),
                &["should be relative", "does not exist"],
            ),
            ("../outside", None, &["'..'"]),
        ] {
            let root = tempfile::tempdir().unwrap();
            let diagnostics = validate(&[symlink(source, None)], root.path());
            assert_messages(&diagnostics, count, messages);
        }
    }

    #[test]
    fn validation_rejects_paths_that_only_name_their_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("symlinks")).unwrap();
        std::fs::write(root.path().join("symlinks/file"), "content").unwrap();

        for path in ["", ".", "./", "././"] {
            for (source, target, code) in [
                (path, ".safe", "symlink.root-source"),
                ("file", path, "symlink.root-target"),
            ] {
                let entry = symlink(source, Some(target));
                assert!(validate_paths(&entry).is_err(), "{entry:?}");
                let diagnostics = validate(&[entry], root.path());
                assert_eq!(diagnostics.len(), 1, "{path:?}: {diagnostics:?}");
                assert_eq!(diagnostics[0].code.to_string(), code);
                assert_eq!(diagnostics[0].source, SYMLINKS_TOML);
                assert_eq!(diagnostics[0].item, source);
                assert_eq!(
                    diagnostics[0].severity,
                    crate::infra::config::Severity::Error
                );
                assert!(diagnostics[0].message.contains("must name"));
            }
        }
    }

    #[test]
    fn validation_accepts_named_dot_relative_paths_but_not_parents() {
        for (source, target) in [
            ("./bashrc", "./.bashrc"),
            ("./config/./git/config", ".config/./git/config"),
            (".hidden", "..hidden"),
        ] {
            validate_paths(&symlink(source, Some(target))).unwrap();
        }
        for (source, target) in [
            ("./../outside", ".safe"),
            ("file", "./../outside"),
            ("file", r".\..\outside"),
        ] {
            assert!(
                validate_paths(&symlink(source, Some(target))).is_err(),
                "{source} -> {target}"
            );
        }
    }

    #[test]
    fn expand_glob_patterns_rejects_traversal_and_invalid_wildcards() {
        for (source, target, fragment) in [
            ("../*", "../../outside", "must not contain '..'"),
            ("skills/*", ".copilot/skills", "wildcard"),
            ("skills/**", ".copilot/skills/*", "**"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let entries = [symlink(source, Some(target))];
            let err = expand_glob_patterns(&entries, root.path()).unwrap_err();
            assert!(
                err.to_string().contains(fragment),
                "{source} -> {target}: {err}"
            );
        }
    }

    #[test]
    fn multiple_glob_captures_preserve_order_sorting_and_origin() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let origin = fixture.path().join("origin");
        let fallback = fixture.path().join("fallback");
        for (root, relative) in [
            (&origin, "zeta/b.conf"),
            (&origin, "alpha/z.conf"),
            (&origin, "alpha/a.conf"),
            (&fallback, "wrong/ignored.conf"),
        ] {
            let path = root.join("symlinks").join("apps").join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "fixture").unwrap();
        }
        let pattern = Symlink {
            source: "apps/*/*".into(),
            target: Some(".config/*/managed/*".into()),
            allow_external_target: true,
            origin: Some(origin.clone()),
        };

        let expanded = expand_glob_patterns(&[pattern], &fallback).unwrap();

        assert_eq!(
            expanded
                .iter()
                .map(|entry| (entry.source.as_str(), entry.target.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("apps/alpha/a.conf", Some(".config/alpha/managed/a.conf")),
                ("apps/alpha/z.conf", Some(".config/alpha/managed/z.conf")),
                ("apps/zeta/b.conf", Some(".config/zeta/managed/b.conf")),
            ],
        );
        assert!(
            expanded
                .iter()
                .all(|entry| entry.origin.as_ref() == Some(&origin))
        );
        assert!(expanded.iter().all(|entry| entry.allow_external_target));
    }

    #[test]
    fn empty_matches_and_target_only_globs_are_errors_not_silent_omissions() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        std::fs::create_dir(fixture.path().join("symlinks")).unwrap();
        for (source, target, diagnostic) in [
            ("missing/*", None, "matched no entries"),
            ("file", Some(".config/*"), "is not a glob"),
        ] {
            let pattern = symlink(source, target);
            let error = expand_glob_patterns(&[pattern], fixture.path()).unwrap_err();
            assert!(error.to_string().contains(diagnostic), "{error}");
            assert!(error.to_string().contains(source), "{error}");
        }
    }

    #[test]
    fn validate_rejects_absolute_and_traversing_targets() {
        for (target, fragment) in [
            ("/etc/passwd", "should be relative to $HOME directory"),
            ("../../etc/passwd", "'..'"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let sources = root.path().join("symlinks");
            std::fs::create_dir_all(&sources).unwrap();
            std::fs::write(sources.join("bashrc"), "").unwrap();
            let warnings = validate(&[symlink("bashrc", Some(target))], root.path());
            assert_messages(&warnings, Some(1), &[fragment]);
        }
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
        let symlink = symlink("escaped", None);

        let diagnostics = validate(std::slice::from_ref(&symlink), temp_dir.path());

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.to_string(),
            "symlink.source-outside-root"
        );
        assert!(diagnostics[0].message.contains("outside"));
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_source_is_not_reported_as_missing_and_recovers_after_access_is_restored() {
        use crate::infra::config::Severity;
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let dir = fixture.path().join("symlinks/locked");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("config");
        std::fs::write(&source, "custom").unwrap();
        let entries = [symlink("locked/config", None)];
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let diagnostics = validate(&entries, fixture.path());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one source inspection error, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code, SYMLINK_SOURCE_INSPECTION);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.source, "symlinks.toml");
        assert_eq!(diagnostic.item, "locked/config");
        assert!(diagnostic.message.contains(&source.display().to_string()));
        assert!(validate(&entries, fixture.path()).is_empty());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "custom");
        std::fs::remove_file(source).unwrap();
        assert_eq!(
            validate(&entries, fixture.path())[0].code,
            SYMLINK_SOURCE_MISSING
        );
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
        let symlink = symlink("alias", None);

        assert!(validate(&[symlink], temp_dir.path()).is_empty());
    }

    #[test]
    fn validate_unique_targets_rejects_explicit_and_computed_collisions() {
        for declarations in [
            [("bashrc", None), ("other", Some(".bashrc"))],
            [("one", Some(".same")), ("two", Some(".same"))],
        ] {
            let root = tempfile::tempdir().unwrap();
            let sources = root.path().join("symlinks");
            std::fs::create_dir_all(&sources).unwrap();
            for (source, _) in declarations {
                std::fs::write(sources.join(source), "").unwrap();
            }
            let entries = declarations.map(|(source, target)| symlink(source, target));
            let error = validate_unique_targets(&entries).unwrap_err();
            assert!(
                error.to_string().contains("collision"),
                "{entries:?}: {error}"
            );
        }
    }

    #[test]
    fn validate_unique_targets_rejects_ancestor_overlap_in_either_order() {
        let parent = symlink("config/example", None);
        let child = symlink("example-file", Some(".config/example/file"));

        for symlinks in [vec![parent.clone(), child.clone()], vec![child, parent]] {
            let error = validate_unique_targets(&symlinks).unwrap_err();
            assert_mentions(
                &error.to_string(),
                &["overlap", ".config/example", ".config/example/file"],
            );
        }
    }

    #[test]
    fn validate_unique_targets_allows_shared_textual_prefix() {
        let symlinks = vec![
            symlink("one", Some(".config/example")),
            symlink("two", Some(".config/example-extra/file")),
        ];

        validate_unique_targets(&symlinks).unwrap();
    }

    #[test]
    fn expand_glob_patterns_expands_skill_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let skills_dir = temp_dir.path().join("symlinks").join("skills");
        std::fs::create_dir_all(skills_dir.join("alpha")).unwrap();
        std::fs::create_dir_all(skills_dir.join("bravo")).unwrap();

        let symlinks = vec![symlink("skills/*", Some(".copilot/skills/*"))];

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
            allow_external_target: false,
            origin: Some(origin.clone()),
        }];

        let expanded = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].origin.as_deref(), Some(origin.as_path()));
    }
}
