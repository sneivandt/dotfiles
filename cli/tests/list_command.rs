#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "fixture assertions"
)]
//! Read-only command discovery and graph-selection contracts.
mod common;

#[test]
fn graph_dependency_expansion_is_available_only_for_update() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for (graph, selector) in [
        ("update", "symlinks"),
        ("remove", "symlinks"),
        ("check", "config-warnings"),
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
