//! Read-only command discovery and graph-selection contracts.

use crate::common;

#[test]
fn list_displays_agent_packages_with_canonical_selector_and_legacy_alias() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for selector in ["", "agent-packages", "apm", "Agent packages"] {
        for format in ["json", "plain", "table"] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, "list", selector);
            if !selector.is_empty() {
                command.args(["--graph", "update"]);
            }
            let output = command.args(["--format", format]).output().unwrap();
            assert!(
                output.status.success(),
                "selector={selector}, format={format}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if format == "json" {
                let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
                let row = rows
                    .iter()
                    .find(|row| row["selector"] == "agent-packages")
                    .unwrap();
                assert_eq!(row["task"], "Agent packages");
                if !selector.is_empty() {
                    assert_eq!(row["selection"], "requested");
                }
            } else {
                let stdout = String::from_utf8(output.stdout).unwrap();
                assert!(stdout.contains("Agent packages"), "{stdout}");
                assert!(
                    stdout
                        .lines()
                        .any(|line| line.split_whitespace().next() == Some("agent-packages")),
                    "{stdout}"
                );
            }
        }
    }
}

#[test]
fn list_hides_empty_configuration_and_all_restores_the_catalog() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for format in ["json", "plain", "table"] {
        for all in [false, true] {
            let mut command = common::cli_command(repo.root_path(), home.path(), None, "list", "");
            command.args(["--format", format]);
            if all {
                command.arg("--all");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let selectors: Vec<String> = if format == "json" {
                serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
                    .unwrap()
                    .into_iter()
                    .map(|row| row.get("selector").unwrap().as_str().unwrap().to_owned())
                    .collect()
            } else {
                String::from_utf8(output.stdout)
                    .unwrap()
                    .lines()
                    .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
                    .collect()
            };
            for selector in [
                "packages",
                "symlinks",
                "registry",
                "agent-settings",
                "vscode-extensions",
            ] {
                assert_eq!(
                    selectors.iter().any(|value| value == selector),
                    all,
                    "{selector}: {format}, all={all}"
                );
            }
            for selector in ["agent-packages", "launcher", "shellcheck"] {
                assert!(
                    selectors.iter().any(|value| value == selector),
                    "uncertain or unconditional {selector} must remain visible"
                );
            }
            assert!(!home.path().join("logs").exists());
            assert!(!repo.root_path().join(".git/dotfiles-run.lock").exists());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn list_hides_system_tasks_in_ci_unless_all_is_requested() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "systemd-units.toml",
            "[linux]\nunits = ['fixture.service']\n",
        )
        .with_config_file(
            "system-files.toml",
            "[linux]\nfiles = [{ source = 'fixture.toml', merge = 'toml' }]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    for (is_ci, all) in [(false, false), (true, false), (true, true)] {
        let mut command = common::cli_command(repo.root_path(), home.path(), None, "list", "");
        if is_ci {
            command.env("CI", "");
        } else {
            command.env_remove("CI");
        }
        if all {
            command.arg("--all");
        }
        let output = command.args(["--format", "json"]).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        for selector in ["systemd", "system-files"] {
            assert_eq!(
                rows.iter().any(|row| row["selector"] == selector),
                !is_ci || all,
                "{selector}: CI={is_ci}, all={all}"
            );
        }
    }
}

#[test]
fn list_displays_custom_packages_with_canonical_selectors_and_labels() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let output = common::cli_command(repo.root_path(), home.path(), None, "list", "")
        .args(["--all", "--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    for (selector, label) in [
        ("custom-package-versions", "Custom package versions"),
        ("custom-package-repository", "Custom package repository"),
    ] {
        let row = rows.iter().find(|row| row["selector"] == selector).unwrap();
        assert_eq!(row["task"], label);
    }
    assert!(!rows.iter().any(|row| {
        row["selector"]
            .as_str()
            .unwrap()
            .starts_with("local-package-")
    }));
}

#[test]
fn list_keeps_configured_work_and_validates_sources_outside_the_profile() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.root_path().join("conf/vscode-extensions.toml"),
        "[base]\nextensions = [\"example.extension\"]\n",
    )
    .unwrap();
    std::fs::write(
        repo.root_path().join("conf/symlinks.toml"),
        "[desktop]\nsymlinks = [\"example\"]\n",
    )
    .unwrap();
    let output = common::cli_command(repo.root_path(), home.path(), None, "list", "")
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        rows.iter()
            .any(|row| row["selector"] == "vscode-extensions")
    );
    assert!(rows.iter().any(|row| row["selector"] == "symlink-sources"));
    assert!(!rows.iter().any(|row| row["selector"] == "symlinks"));
    assert!(!home.path().join("logs").exists());
}

#[test]
fn graph_dependency_expansion_is_available_only_for_update() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for (graph, selector) in [
        ("update", "symlinks"),
        ("remove", "symlinks"),
        ("check", "config-diagnostics"),
    ] {
        for with_deps in [false, true] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, "list", selector);
            command.args(["--graph", graph, "--format", "json"]);
            if with_deps {
                command.arg("--with-deps");
            }
            let output = command.output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let allowed = graph == "update" || !with_deps;
            assert_eq!(
                output.status.success(),
                allowed,
                "graph={graph}, with_deps={with_deps}: {stderr}"
            );
            if allowed {
                let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
                assert!(
                    rows.iter()
                        .any(|row| row["selector"] == selector && row["selection"] == "requested")
                );
                if graph == "update" && with_deps {
                    assert!(rows.iter().any(|row| row["selection"] == "dependency"));
                }
            } else {
                assert!(
                    stderr.contains("--with-deps is only available for update graphs"),
                    "{stderr}"
                );
            }
            assert!(
                !home.path().join("logs").exists(),
                "listing must not create run logs"
            );
            assert!(
                !repo.root_path().join(".git/dotfiles-run.lock").exists(),
                "listing must not acquire a run lock"
            );
        }
    }
}

#[test]
fn renamed_tasks_accept_legacy_selectors_and_labels_in_both_filter_flags() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for (graph, canonical, legacy) in [
        ("update", "agent-packages", "APM"),
        ("update", "custom-package-repository", "paru-config"),
        ("update", "custom-package-repository", "Paru configuration"),
        (
            "update",
            "custom-package-repository",
            "local-package-repository",
        ),
        (
            "update",
            "custom-package-repository",
            "Local package repository",
        ),
        ("update", "login-shell", "shell"),
        ("update", "login-shell", "Default shell"),
        (
            "update",
            "custom-package-versions",
            "local-package-versions",
        ),
        (
            "update",
            "custom-package-versions",
            "Local package versions",
        ),
        ("update", "custom-package-versions", "Local package updates"),
        ("update", "git", "Git settings"),
        ("update", "registry", "Windows registry"),
        ("update", "system-files", "System files"),
        ("check", "config-diagnostics", "config-warnings"),
        ("check", "config-diagnostics", "Validate config warnings"),
    ] {
        for flag in ["--only", "--skip"] {
            let run = |filter: &str| {
                let output = common::cli_command(repo.root_path(), home.path(), None, "list", "")
                    .args(["--graph", graph, "--format", "json", flag, filter])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{graph} {flag} {filter}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
            };
            let expected = run(canonical);
            assert_eq!(run(legacy), expected, "{graph} {flag} {legacy}");
            let row = expected
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["selector"] == canonical)
                .unwrap();
            assert_eq!(
                row["selection"],
                if flag == "--only" {
                    "requested"
                } else {
                    "skipped"
                }
            );
        }
    }
}

#[test]
fn explain_uses_the_active_overlay_declaration_without_writing_state() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = ['other']\n")
        .with_symlink_source_content("other", "public")
        .build();
    let overlay = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join("conf")).unwrap();
    std::fs::create_dir(overlay.path().join("symlinks")).unwrap();
    let platform = if cfg!(windows) { "windows" } else { "linux" };
    std::fs::write(
        overlay.path().join("conf/symlinks.toml"),
        format!("[base]\nsymlinks = [{{ source = 'fixture', target = '.decoy' }}]\n[base-{platform}]\nsymlinks = [{{ source = 'fixture', target = 'alias/settings' }}]\n"),
    )
    .unwrap();
    std::fs::write(overlay.path().join("symlinks/fixture"), "desired").unwrap();
    let actual = home.path().join("actual");
    let alias = home.path().join("alias");
    std::fs::create_dir(&actual).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&actual, &alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let output = common::cli_command(
        repo.root_path(),
        home.path(),
        Some(overlay.path()),
        "explain",
        "",
    )
    .arg(actual.join("settings"))
    .output()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let explain_field = |label: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(label).map(str::trim))
    };
    assert_eq!(explain_field("Profile:"), Some("base"), "{text}");
    assert!(explain_field("Active overlay:").is_some(), "{text}");
    assert!(
        text.contains(&format!("symlinks.toml [base-{platform}]")),
        "{text}"
    );
    assert_eq!(
        explain_field("Update:"),
        Some("create managed link"),
        "{text}"
    );
    assert_eq!(
        explain_field("Remove:"),
        Some("leave target as it is"),
        "{text}"
    );
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 2);
    assert!(!actual.join("settings").exists());
    assert!(!repo.root_path().join(".git/dotfiles-run.lock").exists());
}

#[test]
fn explain_rechecks_aliases_and_rejects_collisions_after_a_match() {
    #[cfg(unix)]
    use std::os::unix::fs::{symlink as link_dir, symlink as link_file};
    #[cfg(windows)]
    use std::os::windows::fs::{symlink_dir as link_dir, symlink_file as link_file};

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks=['fixture', {source='fixture', target='alias/item'}, \
             {source='fixture', target='second/item'}]\n",
        )
        .with_symlink_source_content("fixture", "desired")
        .build();
    let home = tempfile::tempdir().unwrap();
    let first = home.path().join("first");
    let second = home.path().join("second");
    let alias = home.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    link_dir(&first, &alias).unwrap();
    let explain = |path: &str| {
        common::cli_command(repo.root_path(), home.path(), None, "explain", "")
            .arg(path)
            .output()
            .unwrap()
    };
    let matched = explain("first/item");
    assert!(matched.status.success(), "{matched:?}");
    let text = String::from_utf8(matched.stdout).unwrap();
    let target = text
        .lines()
        .find_map(|line| line.strip_prefix("Target:").map(str::trim));
    assert_eq!(
        target.map(std::path::Path::new),
        Some(alias.join("item").as_path()),
        "{text}"
    );

    // The last component names the entry itself, not the thing it points at.
    std::fs::write(home.path().join(".fixture"), "existing file").unwrap();
    link_file(home.path().join(".fixture"), home.path().join("leaf")).unwrap();
    let unknown = explain("leaf");
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8(unknown.stderr)
            .unwrap()
            .contains("no active symlink manages")
    );

    std::fs::rename(&alias, home.path().join("old-alias")).unwrap();
    link_dir(&second, &alias).unwrap();
    let collision = explain(".fixture");
    assert!(!collision.status.success());
    assert!(collision.stdout.is_empty(), "{collision:?}");
    assert!(
        String::from_utf8(collision.stderr)
            .unwrap()
            .contains("symlink target collision")
    );
}

#[test]
fn explain_reports_install_config_home_guard_but_still_explains_removal() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks=['config/probe','other']\n",
        )
        .with_symlink_source_content("config/probe", "config")
        .with_symlink_source_content("other", "other")
        .build();
    let home = tempfile::tempdir().unwrap();
    let source = repo.root_path().join("symlinks/other");
    let target = home.path().join(".other");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &target).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&source, &target).unwrap();

    // The guard covers the whole install task, not just paths under .config.
    for (path, removal) in [
        (".other", "replace link with an independent copy"),
        (".config/probe", "leave target as it is"),
    ] {
        let result = common::cli_command(repo.root_path(), home.path(), None, "explain", "")
            .env("XDG_CONFIG_HOME", home.path().join("elsewhere"))
            .arg(path)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        let text = String::from_utf8(result.stdout).unwrap();
        let field = |label: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(label).map(str::trim))
        };
        assert!(
            field("Update:")
                .is_some_and(|value| value.starts_with("blocked (symlink.unsupported-config-home:")),
            "{text}"
        );
        assert_eq!(field("Remove:"), Some(removal), "{text}");
    }
    assert_eq!(std::fs::read_link(target).unwrap(), source);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 1);
}
