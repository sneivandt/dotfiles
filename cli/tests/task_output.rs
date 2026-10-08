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
        for expected in ["CHANGE Shell completions", "No changes · 1 current"] {
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
        for strict in [false, true] {
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
