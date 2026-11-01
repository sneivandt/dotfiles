#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "fixture assertions"
)]
//! Console and exit-status regressions for task outcomes.

mod common;

#[test]
fn completions_report_changes_then_current() {
    for repository_child in [false, true] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            Some(overlay.path()),
            "update",
            "completions",
        );
        command.arg("--no-symbols");
        if repository_child {
            command
                .env("DOTFILES_REEXEC_GUARD", "1")
                .env("DOTFILES_REPOSITORY_REEXEC_GUARD", "1");
        }
        for expected in ["CHANGED Shell completions", "No changes · 1 task current"] {
            let output = command.output().unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(
                output.status.success(),
                "{repository_child}: {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(stdout.contains(expected), "{repository_child}: {stdout}");
        }
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
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "update",
                "script-audit-script",
            );
            command.args(["--no-symbols", "--fail-on-skip"]);
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
        for (policy, strict, ci, expected_code) in [
            ("default", false, false, 0),
            ("explicit strictness", true, false, 1),
            ("CI presence", false, true, 1),
        ] {
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "check",
                selector,
            );
            command.arg("--no-symbols").env("PATH", empty_path.path());
            if strict {
                command.arg("--fail-on-skip");
            }
            if ci {
                command.env("CI", "");
            }
            let output = command.output().unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(
                output.status.code(),
                Some(expected_code),
                "{selector}, {policy}: {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                stdout.contains(&format!("{tool} not found in PATH")),
                "{stdout}"
            );
            assert!(
                stdout.contains(if expected_code == 1 {
                    "1 task failed"
                } else {
                    "1 task skipped"
                }),
                "{stdout}"
            );
        }
    }
}

#[test]
fn structured_runs_report_success_preview_and_failure_without_human_output() {
    for (verb, selector, dry_run, expected_status) in [
        ("update", "completions", false, "changed"),
        ("update", "completions", true, "dry_run"),
        ("check", "config-diagnostics", false, "passed"),
        ("remove", "completions", false, "error"),
    ] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let mut command = common::cli_command(repo.root_path(), home.path(), None, verb, selector);
        command.args(["--format", "jsonl"]);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let records: Vec<serde_json::Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).expect("valid JSON record"))
            .collect();
        assert!(!records.is_empty(), "{verb}: {stdout}");
        assert_eq!(records.first().unwrap()["type"], "run_start");
        let run_id = records.first().unwrap()["run_id"].as_str().unwrap();
        for record in &records {
            assert_eq!(record["schema"], 1);
            assert_eq!(record["run_id"], run_id);
        }
        let finish = records.last().unwrap();
        assert_eq!(finish["type"], "run_finish");
        assert_eq!(finish["exit_code"], output.status.code().unwrap());
        assert!(!stdout.contains('\u{1b}'));
        if expected_status == "error" {
            assert!(!output.status.success());
            assert_eq!(finish["outcome"], "failed");
            assert!(
                records
                    .iter()
                    .any(|record| record["type"] == "message" && record["event"] == "error")
            );
        } else {
            assert!(
                output.status.success(),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                records.iter().any(|record| record["type"] == "task_result"
                    && record["selector"] == selector
                    && record["status"] == expected_status),
                "{stdout}"
            );
            if dry_run {
                for destination in [
                    repo.root_path()
                        .join("symlinks/config/zsh/completions/_dotfiles"),
                    home.path()
                        .join(".config/powershell/profile.d/dotfiles-completions.ps1"),
                ] {
                    assert!(
                        !destination.exists(),
                        "preview wrote {}",
                        destination.display()
                    );
                }
            }
        }
    }
}

#[test]
fn structured_output_survives_unavailable_persistent_logging_and_startup_errors() {
    let repo = common::TestContextBuilder::new().build();
    let home = tempfile::tempdir().unwrap();
    let blocked = home.path().join("not-a-directory");
    std::fs::write(&blocked, "fixture").unwrap();
    std::fs::write(repo.root_path().join("conf/packages.toml"), "[").unwrap();
    let output = common::cli_command(repo.root_path(), home.path(), None, "update", "completions")
        .args(["--format", "jsonl"])
        .env("DOTFILES_LOG_DIR", &blocked)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let records: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.first().unwrap()["type"], "run_start");
    assert_eq!(records.last().unwrap()["type"], "run_finish");
    assert_eq!(records.last().unwrap()["exit_code"], 1);
}

#[test]
fn quiet_output_retains_failures_and_persistent_records() {
    for selector in ["completions", "unknown-fixture-task"] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let output = common::cli_command(repo.root_path(), home.path(), None, "update", selector)
            .arg("--quiet")
            .output()
            .unwrap();
        assert!(
            output.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        if selector == "completions" {
            assert!(output.status.success());
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("did not match a task selector")
            );
        }
        let logs = common::retained_logs(&home.path().join("logs"));
        assert_eq!(logs.len(), 1);
        let contents = std::fs::read_to_string(logs.first().unwrap()).unwrap();
        assert!(contents.contains("run_finish"));
        if selector == "completions" {
            assert!(contents.contains("task_result"));
        }
    }
}

#[cfg(unix)]
#[test]
fn discovery_and_execution_tolerate_consumers_closing_stdout() {
    use std::os::fd::OwnedFd;
    for verb in ["list", "update"] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
        drop(reader);
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            verb,
            if verb == "update" { "completions" } else { "" },
        );
        if verb == "list" {
            command.args(["--format", "json"]);
        }
        let fd: OwnedFd = writer.into();
        let output = command
            .stdout(std::process::Stdio::from(fd))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{verb}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("Broken pipe"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn execution_output_errors_fail_the_run_and_are_recorded() {
    for structured in [false, true] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "update", "completions");
        if structured {
            command.args(["--format", "jsonl"]);
        }
        let output = command.stdout(full).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("writing command output"));
        let logs = common::retained_logs(&home.path().join("logs"));
        assert_eq!(logs.len(), 1);
        let contents = std::fs::read_to_string(logs.first().unwrap()).unwrap();
        assert!(contents.contains("\"outcome\":\"failed\""), "{contents}");
        assert!(contents.contains("\"exit_code\":1"), "{contents}");
    }
}

#[cfg(unix)]
#[test]
fn quiet_and_structured_modes_preserve_real_task_failures() {
    for quiet in [false, true] {
        let repo = common::TestContextBuilder::new().build();
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir(overlay.path().join("conf")).unwrap();
        std::fs::write(
            overlay.path().join("conf/scripts.toml"),
            "[base]\nscripts = [{ name = 'Audit script', path = 'check.sh' }]\n",
        )
        .unwrap();
        std::fs::write(
            overlay.path().join("check.sh"),
            "#!/bin/sh\necho fixture-check-error >&2\nexit 2\n",
        )
        .unwrap();
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            Some(overlay.path()),
            "update",
            "script-audit-script",
        );
        if quiet {
            command.arg("--quiet");
        } else {
            command.args(["--format", "jsonl"]);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        if quiet {
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("fixture-check-error"));
        } else {
            let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(records.iter().any(|record| {
                record["type"] == "task_result"
                    && record["status"] == "failed"
                    && record["reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("fixture-check-error"))
            }));
            assert_eq!(records.last().unwrap()["outcome"], "failed");
        }
    }
}

#[test]
fn quiet_mode_keeps_skipped_work_visible_without_changing_exit_policy() {
    let repo = common::TestContextBuilder::new().build();
    let home = tempfile::tempdir().unwrap();
    let empty_path = tempfile::tempdir().unwrap();
    for strict in [false, true] {
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "check", "shellcheck");
        command.arg("--quiet").env("PATH", empty_path.path());
        if strict {
            command.arg("--fail-on-skip");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.success(), !strict);
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("shellcheck not found in PATH"));
    }
}
