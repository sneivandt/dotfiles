#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "fixture assertions"
)]
//! Console and exit-status regressions for task outcomes.

mod common;

use std::process::Command;

fn command(
    repo: &std::path::Path,
    home: &std::path::Path,
    overlay: &std::path::Path,
    verb: &str,
    selector: &str,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .args([
            verb,
            "--profile",
            "base",
            "--only",
            selector,
            "--non-interactive",
            "--no-symbols",
        ])
        .arg("--root")
        .arg(repo)
        .arg("--overlay")
        .arg(overlay)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("DOTFILES_LOG_DIR", home.join("logs"))
        .env("DOTFILES_SKIP_SELF_UPDATE", "1")
        .env_remove("CI")
        .env_remove("LOCALAPPDATA")
        .env_remove("DOTFILES_OVERLAY")
        .env_remove("DOTFILES_REPOSITORY_REEXEC_GUARD")
        .env_remove("DOTFILES_SELF_UPDATE_REEXEC_GUARD")
        .env_remove("DOTFILES_REEXEC_GUARD");
    if verb == "install" {
        command.arg("--no-repo-update");
    }
    command
}

#[test]
fn completions_report_changes_then_current() {
    let repo = common::TestContextBuilder::new().build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let mut command = command(
        repo.root_path(),
        home.path(),
        overlay.path(),
        "install",
        "completions",
    );
    for expected in ["CHANGE Shell completions", "No changes · 1 current"] {
        let output = command.output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains(expected), "{stdout}");
    }
}

#[cfg(unix)]
#[test]
fn overlay_check_failures_are_visible_in_both_schedulers() {
    for missing in [false, true] {
        for sequential in [false, true] {
            let repo = common::TestContextBuilder::new().build();
            let home = tempfile::tempdir().unwrap();
            let overlay = tempfile::tempdir().unwrap();
            std::fs::create_dir(overlay.path().join("conf")).unwrap();
            std::fs::write(
                overlay.path().join("conf/scripts.toml"),
                "[base]\nscripts = [{ name = 'Audit script', path = 'check.sh' }]\n",
            )
            .unwrap();
            if !missing {
                std::fs::write(
                    overlay.path().join("check.sh"),
                    "#!/bin/sh\necho fixture-check-error >&2\nexit 2\n",
                )
                .unwrap();
            }
            let mut command = command(
                repo.root_path(),
                home.path(),
                overlay.path(),
                "install",
                "script-audit-script",
            );
            command.arg("--fail-on-skip");
            if sequential {
                command.arg("--no-parallel");
            }
            for verbose in [false, true] {
                if verbose {
                    command.arg("--verbose");
                }
                let output = command.output().unwrap();
                let stdout = String::from_utf8(output.stdout).unwrap();
                assert!(!output.status.success(), "{stdout}");
                assert!(stdout.contains("FAILED Audit script"), "{stdout}");
                assert!(
                    stdout.contains(if missing {
                        "script not found"
                    } else {
                        "fixture-check-error"
                    }),
                    "{stdout}"
                );
                assert!(!stdout.contains("No changes"), "{stdout}");
                if verbose {
                    assert!(
                        stdout.contains("running 1 task(s): Audit script"),
                        "{stdout}"
                    );
                }
            }
        }
    }
}

#[test]
fn unavailable_check_tools_explain_skips_and_fail_when_required() {
    let repo = common::TestContextBuilder::new().build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let empty_path = tempfile::tempdir().unwrap();
    for (selector, tool) in [
        ("shellcheck", "shellcheck"),
        ("apm-plugins", "apm"),
        ("psscriptanalyzer", "pwsh"),
    ] {
        for strict in [false, true] {
            let mut command = command(
                repo.root_path(),
                home.path(),
                overlay.path(),
                "check",
                selector,
            );
            command.env("PATH", empty_path.path());
            if strict {
                command.arg("--fail-on-skip");
            }
            let output = command.output().unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(
                output.status.success(),
                !strict,
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                stdout.contains(&format!("{tool} not found in PATH")),
                "{stdout}"
            );
            assert!(
                stdout.contains(if strict { "1 failed" } else { "1 skipped" }),
                "{stdout}"
            );
        }
    }
}
