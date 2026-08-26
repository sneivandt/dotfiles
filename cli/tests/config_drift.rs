#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests that verify the manifest and symlinks configurations
//! stay in sync.
//!
//! These tests read the **real** `conf/manifest.toml` and `conf/symlinks.toml`
//! files from the repository and check that:
//!
//! 1. Every non-base section in `symlinks.toml` has a matching section in
//!    `manifest.toml` so sparse checkout can exclude the right files.
//! 2. Every symlink source path in a non-base section is retained either by
//!    `[base]` ownership or by a manifest path in the same section **or any
//!    manifest section whose category tags are a subset** (i.e. the manifest
//!    section always applies whenever the symlink section applies). For
//!    example, a symlink in `[linux-desktop]` may be covered by the `[desktop]`
//!    manifest because the desktop manifest is always present when
//!    linux-desktop is active.
//! 3. Every path listed in `manifest.toml` actually exists in `symlinks/`.

use serde::{Deserialize, Deserializer, de::Error as _};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// TOML structures (mirrors of the types in the library, kept private here so
// this test file is self-contained and doesn't depend on internal types).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SymlinkSection {
    symlinks: Vec<SymlinkEntry>,
}

enum SymlinkEntry {
    Simple(String),
    WithTarget(SymlinkWithTarget),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SymlinkWithTarget {
    source: String,
    #[allow(dead_code, reason = "used conditionally via cfg")]
    target: String,
}

impl<'de> Deserialize<'de> for SymlinkEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match toml::Value::deserialize(deserializer)? {
            toml::Value::String(value) => Ok(Self::Simple(value)),
            value @ toml::Value::Table(_) => value
                .try_into::<SymlinkWithTarget>()
                .map(Self::WithTarget)
                .map_err(D::Error::custom),
            value @ (toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
            | toml::Value::Array(_)) => Err(D::Error::custom(format!(
                "expected symlink string or table, found {}",
                value.type_str()
            ))),
        }
    }
}

impl SymlinkEntry {
    fn source(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::WithTarget(entry) => &entry.source,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSection {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChmodSection {
    permissions: Vec<PermissionEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionEntry {
    mode: String,
    path: String,
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

fn load_permission_sections(path: &Path) -> HashMap<String, ChmodSection> {
    let content = std::fs::read_to_string(path).expect("read chmod.toml");
    toml::from_str(&content).expect("parse chmod.toml")
}

#[test]
fn mirrored_symlink_parser_rejects_unknown_table_fields() {
    let typo = r#"
        [base]
        symlinks = [{ soruce = "git/.gitconfig", target = "~/.gitconfig" }]
    "#;

    let result = toml::from_str::<HashMap<String, SymlinkSection>>(typo);

    assert!(
        result.is_err(),
        "misspelled symlink keys must not be ignored"
    );
}

#[test]
fn mirrored_section_parsers_reject_unknown_fields() {
    let typo = r#"
        [desktop]
        path = ["config/example"]
    "#;

    let result = toml::from_str::<HashMap<String, ManifestSection>>(typo);

    assert!(
        result.is_err(),
        "misspelled section keys must not be ignored"
    );
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
fn section_tags(section: &str) -> HashSet<&str> {
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

/// Every symlink source in a non-base section must either also be base-owned or
/// be covered by a manifest path in the same section or a compatible subset.
#[test]
fn non_base_symlink_sources_covered_by_manifest() {
    let root = repo_root();
    let conf = root.join("conf");

    let symlinks = load_symlink_sections(&conf.join("symlinks.toml"));
    let manifest = load_manifest_sections(&conf.join("manifest.toml"));
    let base_sources = symlinks.get("base");

    let mut uncovered: Vec<String> = Vec::new();

    for (section, sources) in &symlinks {
        if section == "base" {
            continue;
        }
        for source in sources {
            let retained_by_base =
                base_sources.is_some_and(|base_entries| base_entries.contains(source));
            if !retained_by_base && !is_covered_by_any_section(section, source, &manifest) {
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

/// VS Code's platform-specific and remote links all use the shared
/// `config/Code` sources, so every desktop ownership combination must retain
/// that directory.
#[test]
fn vscode_shared_sources_are_retained_for_every_desktop_platform() {
    let manifest = load_manifest_sections(&repo_root().join("conf").join("manifest.toml"));
    let targets = [
        "config/Code/User/keybindings.json",
        "config/Code/User/settings.json",
    ];

    for section in ["desktop", "linux-desktop", "windows-desktop"] {
        for target in targets {
            assert!(
                is_covered_by_any_section(section, target, &manifest),
                "[{section}] shared VS Code source is excluded: {target}"
            );
        }
    }
}

#[test]
fn vscode_insiders_desktop_launchers_use_gnome_libsecret() {
    let applications = repo_root().join("symlinks/local/share/applications");

    for file in ["code-insiders.desktop", "code-insiders-url-handler.desktop"] {
        let content =
            std::fs::read_to_string(applications.join(file)).expect("read VS Code launcher");
        let commands: Vec<&str> = content
            .lines()
            .filter_map(|line| line.strip_prefix("Exec="))
            .collect();

        assert!(!commands.is_empty(), "{file} has no Exec entries");
        for command in commands {
            assert!(
                command
                    .split_ascii_whitespace()
                    .any(|arg| arg == "--password-store=gnome-libsecret"),
                "{file} Exec entry does not select gnome-libsecret: {command}"
            );
        }
    }
}

#[test]
fn hypr_vscode_launcher_resolves_managed_code_insiders_shim() {
    let hypr = repo_root().join("symlinks/config/hypr");
    let binds = std::fs::read_to_string(hypr.join("conf/binds.lua")).expect("read Hypr bindings");
    let chooser = std::fs::read_to_string(hypr.join("scripts/choose-editor.sh"))
        .expect("read editor chooser");
    let path = std::fs::read_to_string(repo_root().join("symlinks/config/shell/path.sh"))
        .expect("read PATH");
    let symlinks = load_symlink_sections(&repo_root().join("conf/symlinks.toml"));

    assert!(
        binds.contains(
            r#"hl.bind(mod .. " + v", hl.dsp.exec_cmd("~/.config/hypr/scripts/choose-editor.sh"))"#
        ),
        "Super+V does not invoke the managed editor chooser"
    );
    assert!(
        chooser.contains("for editor in code-insiders code gvim")
            && chooser.contains("exec \"$editor\""),
        "editor chooser does not resolve code-insiders through PATH before its fallbacks"
    );
    assert!(
        path.contains(r#"_path_prepend "$HOME/.local/bin""#),
        "managed user bin is not configured on PATH"
    );
    assert!(
        symlinks.get("arch-desktop").is_some_and(|sources| sources
            .iter()
            .any(|source| source == "local/bin/code-insiders")),
        "Arch desktop does not install the code-insiders shim"
    );
}

#[cfg(unix)]
#[test]
fn code_insiders_shim_normalizes_password_store_and_forwards_arguments() {
    use std::os::unix::fs::PermissionsExt as _;

    let source_path = repo_root().join("symlinks/local/bin/code-insiders");
    let source = std::fs::read_to_string(&source_path).expect("read code-insiders shim");
    let temp = tempfile::tempdir().expect("create shim test directory");
    let fake_binary = temp.path().join("code-insiders-real");
    let test_shim = temp.path().join("code-insiders");
    let captured = temp.path().join("arguments");

    std::fs::write(
        &fake_binary,
        "#!/bin/sh\n: > \"$CAPTURE_ARGS\"\nfor argument do\n  printf '%s\\n' \"$argument\" >> \"$CAPTURE_ARGS\"\ndone\n",
    )
    .expect("write fake code-insiders binary");
    let patched = source.replace(
        r#"REAL_BINARY = "/usr/bin/code-insiders""#,
        &format!("REAL_BINARY = {:?}", fake_binary.to_string_lossy()),
    );
    assert_ne!(
        patched, source,
        "shim real binary constant was not replaced"
    );
    std::fs::write(&test_shim, patched).expect("write test code-insiders shim");

    for executable in [&fake_binary, &test_shim] {
        let mut permissions = std::fs::metadata(executable)
            .expect("read executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(executable, permissions).expect("make test file executable");
    }

    let cases = [
        (
            vec![
                "--new-window",
                "project with spaces",
                "--password-store=kwallet6",
                "--verbose",
            ],
            vec![
                "--password-store=gnome-libsecret",
                "--new-window",
                "project with spaces",
                "--verbose",
            ],
        ),
        (
            vec![
                "--password-store",
                "basic",
                "--reuse-window",
                "workspace.code-workspace",
            ],
            vec![
                "--password-store=gnome-libsecret",
                "--reuse-window",
                "workspace.code-workspace",
            ],
        ),
        (
            vec![
                "--password-store=gnome-libsecret",
                "--",
                "--password-store=literal-file-name",
            ],
            vec![
                "--password-store=gnome-libsecret",
                "--",
                "--password-store=literal-file-name",
            ],
        ),
    ];

    for (arguments, expected) in cases {
        let status = std::process::Command::new(&test_shim)
            .args(arguments)
            .env("CAPTURE_ARGS", &captured)
            .status()
            .expect("run code-insiders shim");
        assert!(status.success(), "code-insiders shim failed");
        let actual: Vec<String> = std::fs::read_to_string(&captured)
            .expect("read captured arguments")
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(actual, expected);
    }
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

#[test]
fn hypr_helper_scripts_have_executable_permissions() {
    let root = repo_root();
    let scripts_dir = root.join("symlinks/config/hypr/scripts");
    let excluded = sparse_checkout_excluded_paths(&root);
    if is_excluded_by_sparse("config/hypr/scripts/", &excluded) {
        return;
    }

    let configured: HashSet<String> = load_permission_sections(&root.join("conf/chmod.toml"))
        .into_values()
        .flat_map(|section| section.permissions)
        .filter(|entry| entry.mode == "755")
        .map(|entry| entry.path)
        .collect();

    let mut missing = Vec::new();
    for entry in std::fs::read_dir(&scripts_dir).expect("read Hypr helper directory") {
        let path = entry.expect("read Hypr helper entry").path();
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path).expect("read Hypr helper");
        if !content.starts_with("#!") {
            continue;
        }
        let relative = path
            .strip_prefix(root.join("symlinks"))
            .expect("Hypr helper should be under symlinks")
            .to_string_lossy()
            .replace('\\', "/");
        if !configured.contains(&relative) {
            missing.push(relative);
        }
    }

    assert!(
        missing.is_empty(),
        "Hypr helper scripts missing executable chmod entries:\n  {}",
        missing.join("\n  ")
    );
}
