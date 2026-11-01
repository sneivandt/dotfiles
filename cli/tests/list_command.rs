#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "fixture assertions"
)]
//! Read-only command discovery and graph-selection contracts.
mod common;

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
        ("update", "local-package-repository", "paru-config"),
        ("update", "local-package-repository", "Paru configuration"),
        ("update", "login-shell", "shell"),
        ("update", "login-shell", "Default shell"),
        ("update", "local-package-versions", "Local package versions"),
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
