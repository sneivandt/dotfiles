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

use dotfiles_cli::testing as test_api;

use std::sync::Arc;

use test_api::logging::Logger;

// ---------------------------------------------------------------------------
// check command: console output
// ---------------------------------------------------------------------------

#[test]
fn check_console_compacts_only_non_verbose_single_line_tasks() {
    for verbose in [false, true] {
        let ctx = common::TestContextBuilder::new().build();
        std::fs::create_dir_all(ctx.root_path().join(".git")).expect("create .git dir");
        let home = tempfile::tempdir().expect("create temporary home");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_dotfiles"));
        command
            .args([
                "check",
                "--profile",
                "base",
                "--only",
                "config-warnings,config-files",
                "--no-parallel",
                "--non-interactive",
                "--no-symbols",
            ])
            .arg("--root")
            .arg(ctx.root_path())
            .env("HOME", home.path())
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("DOTFILES_LOG_DIR", home.path().join("logs"))
            .env("DOTFILES_SKIP_SELF_UPDATE", "1")
            .env_remove("LOCALAPPDATA")
            .env_remove("DOTFILES_OVERLAY");
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

    let global = test_api::cli::GlobalOpts {
        root: Some(ctx.root_path().to_path_buf()),
        profile: Some("base".to_string()),
        dry_run: true,
        overlay: None,
        parallel: false,
        no_repo_update: false,
        require_complete: false,
        non_interactive: false,
        no_symbols: false,
        skip_attestation: false,
        elevated_child: false,
    };
    let opts = test_api::cli::CheckOpts {
        skip: vec![],
        only: vec![],
        with_deps: false,
    };
    let log = Arc::new(Logger::new("test-command"));
    let runtime = test_api::commands::RuntimePolicy::new(
        &global,
        false,
        test_api::env::MapEnv::new()
            .with("HOME", ctx.root_path().join("home"))
            .with("USERPROFILE", ctx.root_path().join("home"))
            .with("XDG_STATE_HOME", ctx.root_path().join("state"))
            .into_handle(),
        false,
        false,
    );

    let result = test_api::commands::check::run(
        &runtime,
        &opts,
        &log,
        &test_api::engine::CancellationToken::new(),
    );
    assert!(result.is_err(), "test command should fail on warnings");
}
