#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! Integration tests that verify the manifest and symlinks configurations
//! stay in sync.
//!
//! These tests read the **real** `conf/manifest.toml` and `conf/symlinks.toml`
//! files from the repository and check that:
//!
//! 1. Every non-base section in `symlinks.toml` has a matching section in
//!    `manifest.toml` so sparse checkout can exclude the right files.
//! 2. Every symlink source path in a non-base section is covered by at least
//!    one manifest path in the same section **or any manifest section whose
//!    category tags are a subset** (i.e. the manifest section always applies
//!    whenever the symlink section applies).  For example, a symlink in
//!    `[linux-desktop]` may be covered by the `[desktop]` manifest because
//!    the desktop manifest is always present when linux-desktop is active.
//! 3. Every path listed in `manifest.toml` actually exists in `symlinks/`.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// TOML structures (mirrors of the types in the library, kept private here so
// this test file is self-contained and doesn't depend on internal types).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SymlinkSection {
    symlinks: Vec<SymlinkEntry>,
}

#[derive(Deserialize)]
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
    fn source(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::WithTarget { source, .. } => source,
        }
    }
}

#[derive(Deserialize)]
struct ManifestSection {
    paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Repository root (parent of `cli/`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ should have a parent directory")
        .to_path_buf()
}

fn load_symlink_sections(path: &Path) -> HashMap<String, Vec<String>> {
    let content = std::fs::read_to_string(path).expect("read symlinks.toml");
    let raw: HashMap<String, SymlinkSection> =
        toml::from_str(&content).expect("parse symlinks.toml");
    raw.into_iter()
        .map(|(k, v)| {
            let sources = v.symlinks.iter().map(|e| e.source().to_owned()).collect();
            (k, sources)
        })
        .collect()
}

fn load_manifest_sections(path: &Path) -> HashMap<String, Vec<String>> {
    let content = std::fs::read_to_string(path).expect("read manifest.toml");
    let raw: HashMap<String, ManifestSection> =
        toml::from_str(&content).expect("parse manifest.toml");
    raw.into_iter().map(|(k, v)| (k, v.paths)).collect()
}

/// Returns `true` when `source` is covered by at least one manifest path.
///
/// A manifest directory entry (trailing `/`) covers any source whose path
/// falls under that directory — either a file inside it **or** the directory
/// itself (a directory symlink like `config/volume` is covered by the
/// manifest entry `config/volume/`).
/// An exact file entry must match the source exactly.
fn is_covered_by(source: &str, manifest_paths: &[String]) -> bool {
    manifest_paths.iter().any(|mp| {
        mp.strip_suffix('/').map_or_else(
            || source == mp,
            |dir| source == dir || source.starts_with(mp.as_str()),
        )
    })
}

/// Parses a section name into its set of category tags.
///
/// Section names are hyphen-separated category tags, e.g. `linux-desktop`
/// produces `{"linux", "desktop"}`.
fn section_tags(section: &str) -> std::collections::HashSet<&str> {
    section.split('-').collect()
}

/// Returns `true` when `source` in `section` is covered by any manifest
/// section that always applies whenever `section` applies.
///
/// A manifest section `Y` always applies when symlink section `X` applies
/// if Y's tags are a subset of X's tags.  For example, `[desktop]` always
/// applies when `[linux-desktop]` is active, so vscode config files whose
/// sparse-checkout entry lives under `[desktop]` still satisfy coverage for
/// `[linux-desktop]` symlinks.
fn is_covered_by_any_section(
    section: &str,
    source: &str,
    manifest: &HashMap<String, Vec<String>>,
) -> bool {
    let symlink_tags = section_tags(section);
    manifest.iter().any(|(msection, mpaths)| {
        let manifest_tags = section_tags(msection);
        manifest_tags.is_subset(&symlink_tags) && is_covered_by(source, mpaths)
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Every non-base category section in `symlinks.toml` must have a
/// corresponding section in `manifest.toml`.
#[test]
fn non_base_symlink_sections_have_manifest_sections() {
    let root = repo_root();
    let conf = root.join("conf");

    let symlinks = load_symlink_sections(&conf.join("symlinks.toml"));
    let manifest = load_manifest_sections(&conf.join("manifest.toml"));

    let missing: Vec<&str> = symlinks
        .keys()
        .filter(|s| *s != "base")
        .filter(|s| !manifest.contains_key(*s))
        .map(String::as_str)
        .collect();

    assert!(
        missing.is_empty(),
        "non-base symlink sections missing from manifest.toml: {missing:?}"
    );
}

/// Every section in `manifest.toml` must have a corresponding section in
/// `symlinks.toml` so that all manifest exclusion rules have matching symlinks.
#[test]
fn manifest_sections_have_symlink_sections() {
    let root = repo_root();
    let conf = root.join("conf");

    let symlinks = load_symlink_sections(&conf.join("symlinks.toml"));
    let manifest = load_manifest_sections(&conf.join("manifest.toml"));

    let missing: Vec<&str> = manifest
        .keys()
        .filter(|s| !symlinks.contains_key(*s))
        .map(String::as_str)
        .collect();

    assert!(
        missing.is_empty(),
        "manifest.toml sections missing from symlinks.toml: {missing:?}"
    );
}

/// Every symlink source in a non-base section must be covered by a manifest
/// path in the same section or any manifest section whose category tags are a
/// strict subset (i.e. always active when the symlink section is active).
#[test]
fn non_base_symlink_sources_covered_by_manifest() {
    let root = repo_root();
    let conf = root.join("conf");

    let symlinks = load_symlink_sections(&conf.join("symlinks.toml"));
    let manifest = load_manifest_sections(&conf.join("manifest.toml"));

    let mut uncovered: Vec<String> = Vec::new();

    for (section, sources) in &symlinks {
        if section == "base" {
            continue;
        }
        for source in sources {
            if !is_covered_by_any_section(section, source, &manifest) {
                uncovered.push(format!("[{section}] {source}"));
            }
        }
    }

    assert!(
        uncovered.is_empty(),
        "symlink sources not covered by manifest.toml:\n  {}",
        uncovered.join("\n  ")
    );
}

/// Returns paths excluded by sparse checkout (relative to `symlinks/`).
///
/// Reads `info/sparse-checkout` from the git directory and collects negated
/// patterns that start with `!/symlinks/`, stripping the prefix so they match
/// manifest paths.  Handles both normal repos (`.git/` is a directory) and
/// worktrees (`.git` is a file containing `gitdir: <path>`).
fn sparse_checkout_excluded_paths(root: &Path) -> Vec<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        // Worktree: .git is a file like "gitdir: /path/to/.git/worktrees/name"
        let content = std::fs::read_to_string(&dot_git).unwrap_or_default();
        content
            .strip_prefix("gitdir: ")
            .and_then(|s| s.strip_suffix('\n').or(Some(s)))
            .map(|s| PathBuf::from(s.trim()))
            .unwrap_or(dot_git)
    } else {
        dot_git
    };
    let sc_path = git_dir.join("info/sparse-checkout");
    let Ok(content) = std::fs::read_to_string(sc_path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| line.strip_prefix("!/symlinks/"))
        .map(String::from)
        .collect()
}

/// Returns `true` when `path` is excluded by one of the sparse checkout
/// negation patterns (exact match or directory prefix).
fn is_excluded_by_sparse(path: &str, excluded: &[String]) -> bool {
    excluded.iter().any(|ex| {
        ex.strip_suffix('/').map_or_else(
            || path == ex,
            |dir| path == dir || path.starts_with(ex.as_str()),
        )
    })
}

/// Every path listed in `manifest.toml` must correspond to an existing
/// file or directory inside `symlinks/`.
///
/// Paths excluded by sparse checkout are skipped — they are intentionally
/// absent on this machine.
#[test]
fn manifest_paths_exist_in_symlinks_dir() {
    let root = repo_root();
    let symlinks_dir = root.join("symlinks");
    let conf = root.join("conf");

    let manifest = load_manifest_sections(&conf.join("manifest.toml"));
    let excluded = sparse_checkout_excluded_paths(&root);

    let mut missing: Vec<String> = Vec::new();

    for (section, paths) in &manifest {
        for path in paths {
            if is_excluded_by_sparse(path, &excluded) {
                continue;
            }
            let full = symlinks_dir.join(path);
            if !full.exists() {
                missing.push(format!("[{section}] {path}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "manifest paths not found in symlinks/:\n  {}",
        missing.join("\n  ")
    );
}
