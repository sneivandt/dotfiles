#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "fixture assertions"
)]
//! Runtime shell completion, parsing parity, and read-only task discovery.
mod common;

fn candidates(
    repo: &common::IntegrationTestContext,
    home: &std::path::Path,
    shell: &str,
    words: &[&str],
    index: usize,
) -> Vec<String> {
    let mut command = common::cli_command(repo.root_path(), home, None, "list", "");
    command
        .env("DOTFILES_COMPLETE", shell)
        .env("_CLAP_COMPLETE_INDEX", index.to_string())
        .args(["--", "dotfiles"])
        .args(words);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| line.split([':', '\t']).next().unwrap().to_owned())
        .collect()
}

#[test]
fn runtime_formats_and_flag_constraints_match_the_parser() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    for shell in ["zsh", "bash", "fish", "elvish", "powershell"] {
        // Fish/PowerShell complete the last word; the other shells provide an index.
        let words = if matches!(shell, "fish" | "powershell") {
            vec!["list", "--format", ""]
        } else {
            vec!["--format", "", "list"]
        };
        let index = if matches!(shell, "fish" | "powershell") {
            3
        } else {
            2
        };
        assert_eq!(
            candidates(&repo, home.path(), shell, &words, index),
            ["table", "plain", "json"],
            "{shell}"
        );
    }
    for (words, forbidden, allowed) in [
        (vec!["update", "--quiet", "--"], "--format", "--dry-run"),
        (vec!["log", "--list", "--"], "--raw", "--format"),
        (vec!["log", "--"], "--format", "--list"),
    ] {
        let values = candidates(&repo, home.path(), "zsh", &words, words.len());
        assert!(
            !values.contains(&forbidden.to_owned()),
            "{words:?}: {values:?}"
        );
        assert!(
            values.contains(&allowed.to_owned()),
            "{words:?}: {values:?}"
        );
    }
    assert!(!home.path().join("logs").exists());
    assert!(!repo.root_path().join(".git/dotfiles-run.lock").exists());
}

#[test]
fn graph_selectors_use_read_only_discovery_and_graph_membership() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let root = repo.root_path().to_str().unwrap();
    for flag in ["--only", "--skip"] {
        for graph in ["check", "update", "remove"] {
            let words = ["list", "--root", root, "-pbase", "--graph", graph, flag, ""];
            let values = candidates(&repo, home.path(), "zsh", &words, words.len());
            assert!(!values.is_empty(), "{graph} {flag}");
            assert_eq!(values.contains(&"shellcheck".to_owned()), graph == "check");
            assert_eq!(values.contains(&"launcher".to_owned()), graph != "check");
            assert_eq!(values.contains(&"repository".to_owned()), graph == "update");
            assert!(!home.path().join("logs").exists());
            assert!(!repo.root_path().join(".git/dotfiles-run.lock").exists());
        }
    }
}
