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
// test command: warning handling
// ---------------------------------------------------------------------------

/// The `test` command should fail when config validation emits warnings.
#[test]
fn test_command_fails_on_config_warnings() {
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
        offline: false,
        require_complete: false,
        non_interactive: false,
        retry_failed: false,
        no_symbols: false,
        skip_attestation: false,
        elevated_child: false,
    };
    let opts = test_api::cli::TestOpts {
        skip: vec![],
        only: vec![],
    };
    let log = Arc::new(Logger::new("test-command"));

    let result = test_api::commands::test::run(
        &global,
        &opts,
        &log,
        &test_api::engine::CancellationToken::new(),
    );
    assert!(result.is_err(), "test command should fail on warnings");
}
