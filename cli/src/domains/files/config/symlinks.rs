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
pub(crate) fn expand_glob_patterns(symlinks: &[Symlink], fallback: &Path) -> Result<Vec<Symlink>> {
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
pub(crate) fn resolve_symlinks_dir(symlink: &Symlink, fallback: &Path) -> PathBuf {
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
pub(crate) fn validate(symlinks: &[Symlink], root: &Path) -> Vec<Diagnostic> {
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
mod tests;
