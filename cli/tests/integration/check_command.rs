//! Integration tests for the `test` command.
//!
//! Split by concern so a failure names the behaviour rather than one large
//! file:
//! - [`loading`] — `Config::load` and profile resolution, including parse errors
//! - [`validation`] — warnings produced by `Config::validate`
//!
//! This root module keeps the shared imports and the end-to-end behaviour of
//! the command itself.

use crate::common;
mod loading;
mod validation;

// ---------------------------------------------------------------------------
// check command: console output
// ---------------------------------------------------------------------------

#[test]
fn check_console_compacts_only_non_verbose_single_line_tasks() {
    for verbose in [false, true] {
        let ctx = common::TestContextBuilder::new().build();
        std::fs::create_dir_all(ctx.root_path().join(".git")).expect("create .git dir");
        let home = tempfile::tempdir().expect("create temporary home");
        let mut command = common::cli_command(
            ctx.root_path(),
            home.path(),
            None,
            "check",
            "config-diagnostics,config-files",
        );
        command.args(["--no-parallel", "--no-symbols"]);
        if verbose {
            command.arg("--verbose");
        }

        let output = command.output().expect("run isolated check");
        let text = String::from_utf8(output.stdout).expect("check output should be UTF-8");
        assert!(
            output.status.success(),
            "{text}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !text.contains('\u{1b}'),
            "piped output must be plain: {text:?}"
        );
        assert!(!text.contains("\n\n\n"), "extra blank line: {text:?}");
        let blocks: Vec<_> = text.trim().split("\n\n").collect();
        assert_eq!(blocks.len(), if verbose { 4 } else { 3 }, "{text}");
        if verbose {
            assert!(
                blocks[1].starts_with("PASSED Configuration diagnostics"),
                "{text}"
            );
            assert!(
                blocks[2].starts_with("PASSED Required configuration files"),
                "{text}"
            );
        } else {
            assert_eq!(
                blocks[1], "PASSED Configuration diagnostics\nPASSED Required configuration files",
                "{text}"
            );
        }
        assert!(
            blocks.last().unwrap().starts_with("Checks · 2 passed"),
            "{text}"
        );
        let history = |args: &[&str]| {
            let viewed = common::cli_command(ctx.root_path(), home.path(), None, "log", "")
                .args(args)
                .output()
                .unwrap();
            assert!(viewed.status.success(), "{viewed:?}");
            String::from_utf8(viewed.stdout).unwrap()
        };
        let log = history(&[]);
        for task in ["Configuration diagnostics", "Required configuration files"] {
            assert_eq!(log.matches(task).count(), 1, "{log}");
            assert!(log.contains(&format!("{task}: Passed")), "{log}");
        }
        assert_eq!(log.matches("base profile").count(), 1, "{log}");
        assert!(log.lines().any(|line| line == "Checks · 2 passed"), "{log}");
        assert!(log.contains("Result: succeeded"), "{log}");
        let detailed = history(&["--task", "config-files", "-v"]);
        assert!(
            detailed.contains("required config files present"),
            "{detailed}"
        );
        assert!(
            !detailed.contains("PASSED Required configuration files"),
            "{detailed}"
        );
    }
}

// ---------------------------------------------------------------------------
// check command: diagnostics
// ---------------------------------------------------------------------------

/// The `test` command should fail when config validation emits warnings.
#[test]
fn check_command_fails_on_config_warnings() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions = [\"invalid_no_dot\"]\n",
        )
        .build();

    std::fs::create_dir_all(ctx.root_path().join(".git")).expect("create .git dir");

    let home = tempfile::tempdir().unwrap();
    let output = common::cli_command(
        ctx.root_path(),
        home.path(),
        None,
        "check",
        "config-diagnostics",
    )
    .args(["--no-symbols", "--fail-on-skip"])
    .output()
    .unwrap();
    let text = common::combined_output(&output);
    assert!(
        !output.status.success(),
        "check command should fail on warnings: {text}"
    );
    assert!(text.contains("FAILED Configuration diagnostics"), "{text}");
    assert!(
        text.contains("invalid_no_dot") && text.contains("vscode-extensions.toml"),
        "{text}"
    );
    assert!(text.contains("Checks · 1 failed"), "{text}");
}
#[cfg(target_os = "linux")]
#[test]
fn check_and_update_reject_conflicting_system_file_paths_before_tasks_run() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "system-files.toml",
            "[linux]\nfiles = [\
                { target = '/etc/dotfiles-audit-path-fixture', source = 'parent', merge = 'create-only' },\
                { target = '/etc/dotfiles-audit-path-fixture/child', source = 'child', merge = 'create-only' }]\n",
        )
        .build();
    let system = ctx.root_path().join("system");
    std::fs::create_dir(&system).unwrap();
    std::fs::write(system.join("parent"), "parent\n").unwrap();
    std::fs::write(system.join("child"), "child\n").unwrap();
    let home = tempfile::tempdir().unwrap();

    for (verb, task) in [("check", "config-diagnostics"), ("update", "system-files")] {
        let mut command = common::cli_command(ctx.root_path(), home.path(), None, verb, task);
        if verb == "update" {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        let text = common::combined_output(&output);
        assert!(!output.status.success(), "{verb}: {text}");
        assert!(
            text.contains(
                "/etc/dotfiles-audit-path-fixture is a parent directory of /etc/dotfiles-audit-path-fixture/child"
            ),
            "{verb}: {text}"
        );
    }
}

#[test]
fn check_rejects_malformed_system_fragments_without_reading_targets() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "system-files.toml",
            "[linux]\nfiles = [\
                { source = 'broken.toml', merge = 'toml' },\
                { source = 'broken.ini', merge = 'ini' },\
                { source = 'empty-key.ini', merge = 'ini' },\
                { source = 'broken-section.ini', merge = 'ini' },\
                { source = 'broken.pam', merge = 'pam' }]\n",
        )
        .build();
    let system = ctx.root_path().join("system");
    std::fs::create_dir(&system).unwrap();
    for (name, content) in [
        ("broken.toml", "[missing-bracket"),
        ("broken.ini", "key=value"),
        ("empty-key.ini", "[options]\n=value\n"),
        ("broken-section.ini", "[options]\n[broken\n"),
        ("broken.pam", "auth optional"),
    ] {
        std::fs::write(system.join(name), content).unwrap();
    }
    let home = tempfile::tempdir().unwrap();
    let output = common::cli_command(
        ctx.root_path(),
        home.path(),
        None,
        "check",
        "config-diagnostics",
    )
    .output()
    .unwrap();
    let text = common::combined_output(&output);
    assert!(!output.status.success(), "{text}");
    for name in [
        "broken.toml",
        "broken.ini",
        "empty-key.ini",
        "broken-section.ini",
        "broken.pam",
    ] {
        assert!(text.contains(name), "{text}");
    }
    assert!(text.contains("system-file.invalid-fragment"), "{text}");
}
