#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "workflow contract assertions"
)]
//! Scheduling contracts for the required and informational CI jobs.

use serde_yaml_ng::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const RUST_IF: &str = "needs.classify-changes.outputs.run_rust_checks == 'true'";
const BUILD_IF: &str = "needs.classify-changes.outputs.run_build_artifacts == 'true'";
const PROFILE_IF: &str = "needs.classify-changes.outputs.run_profile_integration == 'true'";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn workflow() -> Value {
    serde_yaml_ng::from_str(
        &std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap(),
    )
    .unwrap()
}

fn step<'a>(job: &'a Value, name: &str) -> &'a Value {
    job["steps"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .expect(name)
}

fn needs(job: &Value) -> BTreeSet<&str> {
    job["needs"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|need| need.as_str().unwrap())
        .collect()
}

#[test]
fn rust_checks_and_artifacts_follow_separate_classification_flags() {
    let workflow = workflow();
    let jobs = &workflow["jobs"];
    for name in ["build-linux", "build-windows"] {
        assert_eq!(jobs[name]["if"], BUILD_IF, "{name}");
        assert_eq!(needs(&jobs[name]), BTreeSet::from(["classify-changes"]));
        assert_eq!(
            step(&jobs[name], "Build CI binary")["run"],
            "cargo build --profile ci"
        );
    }
    for name in ["rust-fmt", "audit", "deny", "msrv", "windows-rust-checks"] {
        assert_eq!(jobs[name]["if"], RUST_IF, "{name}");
        assert_eq!(needs(&jobs[name]), BTreeSet::from(["classify-changes"]));
    }
    for name in ["Clippy", "Tests"] {
        assert_eq!(step(&jobs["build-linux"], name)["if"], RUST_IF);
    }
    assert_eq!(
        step(&jobs["build-linux"], "Clippy")["run"],
        "cargo clippy --profile ci --all-targets -- -D warnings"
    );
    assert_eq!(
        step(&jobs["build-linux"], "Tests")["run"],
        "cargo test --profile ci"
    );
    let drift = step(&jobs["build-linux"], "Config drift tests");
    assert_eq!(
        drift["if"],
        "needs.classify-changes.outputs.run_rust_checks != 'true' && needs.classify-changes.outputs.run_profile_integration == 'true'"
    );
    assert_eq!(drift["run"], "cargo test --profile ci --test config_drift");
    assert_eq!(
        jobs["validate-config"]["if"],
        "needs.classify-changes.outputs.run_validate_config == 'true' && needs.build-linux.result == 'success'"
    );

    for (name, flag) in [
        ("integration-linux", PROFILE_IF),
        ("integration-windows", PROFILE_IF),
        ("test-install-uninstall", PROFILE_IF),
        ("test-install-uninstall-windows", PROFILE_IF),
        (
            "test-applications",
            "needs.classify-changes.outputs.run_app_tests == 'true'",
        ),
        (
            "test-applications-windows",
            "needs.classify-changes.outputs.run_app_tests == 'true'",
        ),
        (
            "test-shell-wrapper-linux",
            "needs.classify-changes.outputs.run_wrapper_linux == 'true'",
        ),
        (
            "test-shell-wrapper-windows",
            "needs.classify-changes.outputs.run_wrapper_windows == 'true'",
        ),
        (
            "test-git-hooks",
            "needs.classify-changes.outputs.run_git_hooks == 'true'",
        ),
        (
            "docs",
            "needs.classify-changes.outputs.run_docs_checks == 'true'",
        ),
    ] {
        assert_eq!(jobs[name]["if"], flag, "{name}");
    }
}

#[test]
fn windows_consumers_wait_for_artifacts_not_rust_checks() {
    let workflow = workflow();
    let jobs = &workflow["jobs"];
    for name in [
        "integration-windows",
        "test-install-uninstall-windows",
        "test-applications-windows",
        "test-shell-wrapper-windows",
    ] {
        assert_eq!(
            needs(&jobs[name]),
            BTreeSet::from(["classify-changes", "build-windows"]),
            "{name}"
        );
    }
    let artifact = &jobs["build-windows"];
    let steps = artifact["steps"].as_sequence().unwrap();
    let commands: Vec<_> = steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect();
    assert!(
        !commands
            .iter()
            .any(|run| run.contains("cargo clippy") || run.contains("cargo test")),
        "Windows consumers must not wait for linting or tests"
    );
    let upload = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|action| action.starts_with("actions/upload-artifact@"))
        })
        .unwrap();
    assert_eq!(upload["with"]["name"], "dotfiles-windows");
    assert_eq!(upload["with"]["path"], "cli/target/ci/dotfiles.exe");
    assert_eq!(upload["with"]["retention-days"], 1);
    assert_eq!(
        artifact["defaults"],
        jobs["windows-rust-checks"]["defaults"]
    );
    for setup in [
        "Disable Windows Defender real-time monitoring",
        "Install Rust toolchain",
    ] {
        assert_eq!(
            step(artifact, setup),
            step(&jobs["windows-rust-checks"], setup)
        );
    }
    let cache = |job: &Value| {
        job["steps"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|step| {
                step["uses"]
                    .as_str()
                    .is_some_and(|action| action.starts_with("Swatinem/rust-cache@"))
            })
            .unwrap()
            .clone()
    };
    assert_eq!(cache(artifact), cache(&jobs["windows-rust-checks"]));
    assert_eq!(
        cache(artifact)["with"],
        serde_yaml_ng::from_str::<Value>("workspaces: 'cli -> target'\ncache-on-failure: true\n")
            .unwrap(),
        "preserve dependency caching without workspace-crate or source-keyed caches"
    );
}

#[test]
fn windows_keeps_all_target_clippy_and_host_dependent_tests() {
    let workflow = workflow();
    let checks = &workflow["jobs"]["windows-rust-checks"];
    assert_eq!(
        step(checks, "Clippy")["run"],
        "cargo clippy --profile ci --all-targets -- -D warnings"
    );
    let command = step(checks, "Native Windows tests")["run"]
        .as_str()
        .unwrap();
    let prefix = "cargo test --profile ci --lib ";
    let targets: Vec<_> = command
        .strip_prefix(prefix)
        .unwrap()
        .split("--test ")
        .filter(|value| !value.is_empty())
        .map(str::trim)
        .collect();
    assert_eq!(
        targets,
        [
            "behavioral_ci",
            "install_command",
            "task_execution",
            "task_output",
            "test_command"
        ]
    );
    for target in targets {
        assert!(
            repo_root()
                .join("cli/tests")
                .join(format!("{target}.rs"))
                .is_file()
        );
    }
}

#[test]
fn coverage_filters_windows_before_matrix_expansion() {
    let workflow = workflow();
    let coverage = &workflow["jobs"]["coverage"];
    assert_eq!(
        coverage["if"], RUST_IF,
        "matrix is unavailable in job-level if"
    );
    assert_eq!(coverage["continue-on-error"], true);
    assert_eq!(coverage["runs-on"], "${{ matrix.os }}");
    let expression = coverage["strategy"]["matrix"]["include"].as_str().unwrap();
    let branches = expression.strip_prefix(
        "${{ fromJSON((github.event_name == 'pull_request' || github.event_name == 'workflow_dispatch') && '"
    ).unwrap().strip_suffix("') }}").unwrap();
    let (pr_manual, push) = branches.split_once("' || '").unwrap();
    for (event, json, expected) in [
        ("push", push, vec!["ubuntu-latest"]),
        (
            "pull_request",
            pr_manual,
            vec!["ubuntu-latest", "windows-latest"],
        ),
        (
            "workflow_dispatch",
            pr_manual,
            vec!["ubuntu-latest", "windows-latest"],
        ),
    ] {
        let matrix: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        let runners: Vec<_> = matrix
            .iter()
            .map(|entry| entry["os"].as_str().unwrap())
            .collect();
        assert_eq!(runners, expected, "{event}");
        assert_eq!(matrix[0]["name"], "Linux");
        if matrix.len() == 2 {
            assert_eq!(matrix[1]["name"], "Windows");
        }
    }
    assert_eq!(
        step(coverage, "Generate coverage report")["run"],
        "cargo llvm-cov --profile ci --all-targets --summary-only"
    );
}

#[test]
fn success_gate_requires_every_gating_job_and_graph_is_acyclic() {
    let workflow = workflow();
    let jobs = workflow["jobs"].as_mapping().unwrap();
    let gate = &jobs["ci-success"];
    assert_eq!(gate["if"], "always()");
    assert_eq!(gate["name"], "All CI Checks Passed");
    let required: BTreeSet<_> = jobs
        .keys()
        .map(|name| name.as_str().unwrap())
        .filter(|name| !["ci-success", "coverage", "mutation"].contains(name))
        .collect();
    assert_eq!(needs(gate), required);
    for name in &required {
        assert_ne!(
            jobs[*name]["continue-on-error"], true,
            "{name} must gate CI"
        );
    }

    let mut complete = BTreeSet::new();
    while complete.len() < jobs.len() {
        let ready: Vec<_> = jobs
            .iter()
            .filter_map(|(name, job)| {
                let name = name.as_str().unwrap();
                (!complete.contains(name)
                    && (job["needs"].is_null() || needs(job).is_subset(&complete)))
                .then_some(name)
            })
            .collect();
        assert!(!ready.is_empty(), "cycle or unknown dependency in CI graph");
        complete.extend(ready);
    }
}

#[cfg(unix)]
#[test]
fn success_gate_rejects_failures_and_cancellations_but_accepts_skips() {
    let workflow = workflow();
    let script = step(&workflow["jobs"]["ci-success"], "Check all jobs passed")["run"]
        .as_str()
        .unwrap();
    for results in [
        vec!["success"],
        vec!["success", "skipped"],
        vec!["failure", "skipped"],
        vec!["cancelled", "success"],
        vec!["failure", "cancelled"],
    ] {
        let failure = results.contains(&"failure");
        let cancelled = results.contains(&"cancelled");
        let script = script
            .replace(
                "${{ contains(needs.*.result, 'failure') }}",
                if failure { "true" } else { "false" },
            )
            .replace(
                "${{ contains(needs.*.result, 'cancelled') }}",
                if cancelled { "true" } else { "false" },
            );
        assert!(
            !script.contains("${{"),
            "unhandled expression in gate script"
        );
        let output = std::process::Command::new("bash")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !failure && !cancelled,
            "{results:?}"
        );
    }
}

#[cfg(unix)]
mod classifier {
    use super::{repo_root, workflow};
    use std::collections::BTreeMap;

    const FULL: &str = "run_rust_checks run_build_artifacts run_profile_integration run_app_tests run_git_hooks run_wrapper_linux run_wrapper_windows";
    const RUST: &str = "run_rust_checks run_build_artifacts run_profile_integration run_app_tests run_wrapper_linux run_wrapper_windows";
    const CONFIG: &str = "run_build_artifacts run_profile_integration run_app_tests";

    fn commit(repo: &git2::Repository) -> git2::Oid {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("CI fixture", "ci@test.local").unwrap();
        let parent = repo.head().ok().map(|head| head.peel_to_commit().unwrap());
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "fixture",
            &tree,
            &parent.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn classify(label: &str, event: &str, paths: &[&str]) -> String {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let repo = git2::Repository::init(&root).unwrap();
        let base = commit(&repo);
        for path in paths {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "fixture\n").unwrap();
        }
        let head = commit(&repo);
        let output_file = temp.path().join("outputs");
        let output = std::process::Command::new("sh")
            .arg(repo_root().join(".github/workflows/scripts/linux/classify-ci-changes.sh"))
            .env("DIR", &root)
            .env("GITHUB_OUTPUT", &output_file)
            .env("GITHUB_EVENT_NAME", event)
            .env(
                "BASE_SHA",
                if label == "missing-range" {
                    String::new()
                } else {
                    base.to_string()
                },
            )
            .env("HEAD_SHA", head.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read_to_string(output_file).unwrap()
    }

    #[test]
    fn changed_paths_and_fallbacks_preserve_job_selection() {
        let workflow = workflow();
        let keys: Vec<_> = workflow["jobs"]["classify-changes"]["outputs"]
            .as_mapping()
            .unwrap()
            .keys()
            .map(|key| key.as_str().unwrap())
            .collect();
        for (label, event, paths, selected) in [
            ("docs", "pull_request", vec!["docs/TESTING.md"], "docs_only"),
            (
                "agents",
                "push",
                vec![".agents/skills/example/SKILL.md"],
                "docs_only",
            ),
            ("config", "pull_request", vec!["conf/packages.toml"], CONFIG),
            (
                "symlinks",
                "push",
                vec!["symlinks/config/git/config"],
                CONFIG,
            ),
            ("rust", "pull_request", vec!["cli/src/lib.rs"], RUST),
            (
                "linux-wrapper",
                "push",
                vec!["dotfiles.sh"],
                "run_build_artifacts run_wrapper_linux",
            ),
            (
                "windows-wrapper",
                "pull_request",
                vec!["dotfiles.ps1"],
                "run_build_artifacts run_wrapper_windows",
            ),
            ("hooks", "push", vec!["hooks/pre-commit"], "run_git_hooks"),
            (
                "docs-config",
                "pull_request",
                vec!["docs/TESTING.md", "conf/profiles.toml"],
                CONFIG,
            ),
            (
                "rust-config",
                "push",
                vec!["cli/src/lib.rs", "conf/packages.toml"],
                RUST,
            ),
            (
                "docs-rust",
                "pull_request",
                vec!["README.md", "cli/src/lib.rs"],
                RUST,
            ),
            (
                "hooks-wrapper",
                "pull_request",
                vec!["hooks/pre-commit", "dotfiles.ps1"],
                "run_git_hooks run_build_artifacts run_wrapper_windows",
            ),
            ("workflow", "push", vec![".github/workflows/ci.yml"], FULL),
            ("uncategorized", "pull_request", vec!["unknown"], FULL),
            ("manual", "workflow_dispatch", vec!["README.md"], FULL),
            ("missing-range", "push", vec!["README.md"], FULL),
            ("empty-diff", "pull_request", vec![], FULL),
        ] {
            let contents = classify(label, event, &paths);
            let actual: BTreeMap<_, _> = contents
                .lines()
                .map(|line| line.split_once('=').unwrap())
                .collect();
            assert_eq!(
                actual.len(),
                keys.len(),
                "{label}: unexpected classifier outputs"
            );
            for key in &keys {
                let enabled = selected.split_whitespace().any(|flag| flag == *key)
                    || *key == "run_docs_checks"
                    || (selected != "docs_only"
                        && ["run_lint", "run_validate_config"].contains(key));
                assert_eq!(
                    actual.get(key),
                    Some(&if enabled { "true" } else { "false" }),
                    "{label}: {key}"
                );
            }
        }
    }
}
