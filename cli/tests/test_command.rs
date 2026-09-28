#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `test` command.
//!
//! Split by concern so a failure names the behaviour rather than one large
//! file:
//! - [`loading`] — `Config::load` and profile resolution, including parse errors
//! - [`validation`] — warnings produced by `Config::validate`
//!
//! This root binary keeps the shared imports and the end-to-end behaviour of
//! the command itself.

mod common;
#[path = "test_command/loading.rs"]
mod loading;
#[path = "test_command/validation.rs"]
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
            "config-warnings,config-files",
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
                blocks[1].starts_with("PASSED Validate config warnings"),
                "{text}"
            );
            assert!(
                blocks[2].starts_with("PASSED Validate config files"),
                "{text}"
            );
        } else {
            assert_eq!(
                blocks[1], "PASSED Validate config warnings\nPASSED Validate config files",
                "{text}"
            );
        }
        assert!(blocks.last().unwrap().starts_with("2 passed"), "{text}");
    }
}

// ---------------------------------------------------------------------------
// test command: warning handling
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
        "config-warnings",
    )
    .args(["--no-symbols", "--fail-on-skip"])
    .output()
    .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "check command should fail on warnings: {text}"
    );
    assert!(text.contains("FAILED Validate config warnings"), "{text}");
    assert!(
        text.contains("invalid_no_dot") && text.contains("vscode-extensions.toml"),
        "{text}"
    );
    assert!(text.contains("1 failed"), "{text}");
}
