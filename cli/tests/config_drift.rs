#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for relationships among the repository's real
//! configuration files and managed sources.

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

#[test]
fn hypr_helper_scripts_have_executable_permissions() {
    let root = repo_root();
    let scripts_dir = root.join("symlinks/config/hypr/scripts");

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
