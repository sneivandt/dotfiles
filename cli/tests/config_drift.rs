#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
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

#[test]
fn first_login_service_opts_in_to_waiting_for_update_locks() {
    let unit = include_str!("../../symlinks/config/systemd/user/dotfiles-first-login.service");
    assert!(
        unit.lines()
            .any(|line| line == "Environment=DOTFILES_FIRST_LOGIN_WAIT_FOR_LOCK=1"),
        "the first-login service must wait while the update that queues it holds the locks"
    );
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
fn windows_git_editor_override_requires_the_desktop_package() {
    use dotfiles_cli::testing::{
        config::{Config, profiles},
        platform::{Os, Platform},
    };
    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    for name in ["base", "desktop"] {
        let profile = profiles::resolve(name, platform).unwrap();
        let config = Config::load(&repo_root(), &profile, platform, None).unwrap();
        let override_selected = config
            .symlinks
            .iter()
            .any(|link| link.source == "config/git/windows-desktop");
        let editor_installed = config
            .packages
            .iter()
            .any(|package| package.name == "Microsoft.VisualStudioCode.Insiders");
        assert_eq!(override_selected, name == "desktop", "{name}");
        assert_eq!(override_selected, editor_installed, "{name}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn graphical_services_wait_for_session_ownership_without_pulling_it_in() {
    use dotfiles_cli::testing::{
        config::{Config, profiles},
        platform::{Os, Platform},
    };
    let root = repo_root();
    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
        is_wsl: false,
    };
    let profile = profiles::resolve("desktop", platform).unwrap();
    let config = Config::load(&root, &profile, platform, None).unwrap();
    let mut inspected = 0;
    for unit in config.units {
        let path = root.join("symlinks/config/systemd/user").join(&unit.name);
        if !path.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&path).unwrap();
        if !contents.contains("PartOf=graphical-session.target") {
            continue;
        }
        inspected += 1;
        assert!(
            unit.graphical,
            "{} must defer startup outside a graphical session",
            unit.name
        );
        for line in contents.lines() {
            if let Some((key, value)) = line.split_once('=')
                && ["Wants", "Requires", "BindsTo"].contains(&key.trim())
            {
                assert!(
                    !value
                        .split_whitespace()
                        .any(|target| target == "graphical-session.target"),
                    "{} must not pull in graphical-session.target: {line}",
                    unit.name
                );
            }
        }
    }
    assert!(inspected > 0);
}

#[cfg(target_os = "linux")]
#[test]
fn arch_desktop_leaves_dhcpcd_handoff_manual() {
    use dotfiles_cli::testing::{
        config::{Config, profiles},
        platform::{Os, Platform},
    };

    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
        is_wsl: false,
    };
    let profile = profiles::resolve("desktop", platform).expect("desktop profile");
    let config =
        Config::load(&repo_root(), &profile, platform, None).expect("load repository config");

    assert!(
        config
            .units
            .iter()
            .any(|unit| unit.name == "NetworkManager.service" && unit.enabled),
        "the Arch desktop profile should enable NetworkManager"
    );
    assert!(
        config
            .units
            .iter()
            .all(|unit| unit.name != "dhcpcd.service"),
        "the default update must leave dhcpcd alone until the user verifies the network handoff"
    );
}

#[test]
fn vscode_remote_settings_cover_stable_and_insiders_servers() {
    let root = repo_root();
    let content =
        std::fs::read_to_string(root.join("conf/symlinks.toml")).expect("read symlinks.toml");
    let sections: HashMap<String, SymlinkSection> =
        toml::from_str(&content).expect("parse symlinks.toml");
    let source = "config/Code/User/settings.json";
    let targets: HashSet<&str> = sections
        .get("linux-desktop")
        .expect("linux-desktop symlink section")
        .symlinks
        .iter()
        .filter_map(|entry| match entry {
            SymlinkEntry::WithTarget(entry)
                if entry.source == source && entry.target.starts_with(".vscode-server") =>
            {
                Some(entry.target.as_str())
            }
            SymlinkEntry::Simple(_) | SymlinkEntry::WithTarget(_) => None,
        })
        .collect();

    assert_eq!(
        targets,
        HashSet::from([
            ".vscode-server/data/Machine/settings.json",
            ".vscode-server-insiders/data/Machine/settings.json",
        ]),
        "remote settings must target both current VS Code server directories on Linux"
    );
    for section_name in ["desktop", "windows", "windows-desktop"] {
        let has_remote_server = sections.get(section_name).is_some_and(|section| {
            section.symlinks.iter().any(|entry| match entry {
                SymlinkEntry::WithTarget(entry) => entry.target.starts_with(".vscode-server"),
                SymlinkEntry::Simple(path) => path.starts_with("vscode-server"),
            })
        });
        assert!(
            !has_remote_server,
            "Linux-only VS Code remote server settings must not appear in [{section_name}]"
        );
    }
    assert!(
        root.join("symlinks").join(source).is_file(),
        "remote settings source must exist"
    );
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
fn thunar_view_helper_checks_and_applies_the_same_settings() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;

    let root = repo_root();
    let source = "config/Thunar/managed-view-settings.sh";
    let symlinks = load_symlink_sections(&root.join("conf/symlinks.toml"));
    assert!(
        symlinks
            .get("arch-desktop")
            .expect("arch-desktop section exists")
            .iter()
            .any(|entry| entry == source)
    );
    let unit = std::fs::read_to_string(
        root.join("symlinks/config/systemd/user/thunar-view-settings.service"),
    )
    .unwrap();
    assert!(
        unit.lines()
            .any(|line| { line == format!(r#"ExecStart=/bin/sh "%h/.{source}" apply"#) })
    );

    let dir = tempfile::tempdir().unwrap();
    let xfconf = dir.path().join("xfconf-query");
    let settings = dir.path().join("settings");
    std::fs::write(
        &xfconf,
        r#"#!/bin/sh
case "$*" in
  '--channel thunar --list --verbose') /bin/cat "$THUNAR_FIXTURE" ;;
  '--channel thunar --property /last-show-hidden --create --type bool --set true')
    printf '/last-show-hidden    true\n' >> "$THUNAR_FIXTURE" ;;
  '--channel thunar --property /default-view --create --type string --set ThunarCompactView')
    printf '/default-view        ThunarCompactView\n' >> "$THUNAR_FIXTURE" ;;
  '--channel thunar --property /last-side-pane --create --type string --set THUNAR_SIDEPANE_TYPE_HIDDEN_SHORTCUTS')
    printf '/last-side-pane      THUNAR_SIDEPANE_TYPE_HIDDEN_SHORTCUTS\n' >> "$THUNAR_FIXTURE" ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&xfconf, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |mode| {
        Command::new("/bin/sh")
            .arg(root.join("symlinks").join(source))
            .arg(mode)
            .env("PATH", dir.path())
            .env("THUNAR_FIXTURE", &settings)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(run("check"), Some(2), "unreadable settings");
    std::fs::write(&settings, "").unwrap();
    assert_eq!(run("check"), Some(1), "settings drift");
    assert!(std::fs::read(&settings).unwrap().is_empty());
    std::fs::write(
        &settings,
        "/last-show-hidden    true\n/default-view        ThunarCompactView\n/last-side-pane      THUNAR_SIDEPANE_TYPE_SHORTCUTS\n",
    )
    .unwrap();
    assert_eq!(run("check"), Some(1), "visible shortcuts side pane drifts");
    std::fs::write(&settings, "").unwrap();
    assert_eq!(run("apply"), Some(0));
    let applied = std::fs::read(&settings).unwrap();
    assert_eq!(run("check"), Some(0), "matching settings");
    assert_eq!(std::fs::read(&settings).unwrap(), applied);
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
        // Avoid executing a freshly written script directly: parallel test
        // processes can briefly inherit its writer and cause ETXTBSY.
        let status = std::process::Command::new("python3")
            .arg(&test_shim)
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
fn managed_helper_scripts_have_git_tracked_executable_permissions() {
    let root = repo_root();
    let directories = ["symlinks/config/hypr/scripts", "symlinks/local/bin"];
    let output = std::process::Command::new("git")
        .current_dir(&root)
        .args(["ls-files", "--stage", "-z", "--"])
        .args(directories)
        .output()
        .expect("read Git-tracked helper modes");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracked = String::from_utf8(output.stdout).expect("Git helper paths are UTF-8");
    let executable: HashSet<&str> = tracked
        .split('\0')
        .filter(|entry| entry.starts_with("100755 "))
        .map(|entry| {
            entry
                .split_once('\t')
                .expect("Git index entry has a path")
                .1
        })
        .collect();

    let mut missing = Vec::new();
    let entries = directories.into_iter().flat_map(|directory| {
        std::fs::read_dir(root.join(directory)).expect("read managed helper directory")
    });
    for entry in entries {
        let path = entry.expect("read managed helper entry").path();
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path).expect("read managed helper");
        if !content.starts_with("#!") {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .expect("managed helper should be under the repository")
            .to_string_lossy()
            .replace('\\', "/");
        if !executable.contains(relative.as_str()) {
            missing.push(relative);
        }
    }

    assert!(
        missing.is_empty(),
        "Managed helper scripts missing Git-tracked executable bits:\n  {}",
        missing.join("\n  ")
    );
}
