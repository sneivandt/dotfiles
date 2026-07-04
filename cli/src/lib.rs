#![allow(
    unreachable_pub,
    reason = "internal modules are hidden behind the binary entry point and internal-api facade"
)]

//! Dotfiles management engine library entry point.
//!
//! Cross-platform tool for declarative dotfile installation: symlinks,
//! packages, permissions, systemd units, registry entries, VS Code extensions,
//! and AI plugin manifests (via Microsoft APM) — all driven by TOML
//! configuration files in `conf/` and filtered by profile and platform.
//!
//! The stable public API is intentionally small: [`run`] executes the CLI.
//! Engine internals remain crate-private so implementation details can evolve
//! without becoming an accidental library contract.

use std::io::Write as _;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};

mod cli;
mod commands;
mod config;
mod elevation;
mod engine;
mod error;
mod exec;
mod fs;
mod logging;
mod platform;
mod resources;
mod tasks;

#[cfg(any(feature = "internal-api", doctest))]
#[doc(hidden)]
pub mod testing;

/// Run the dotfiles CLI and return the process exit code.
///
/// This is the only supported public entry point for the library crate.  The
/// binary target delegates here so argument parsing, logging setup, graceful
/// cancellation, elevation handling, and command dispatch live in one place.
#[must_use]
pub fn run() -> ExitCode {
    drop(enable_ansi_support::enable_ansi_support()); // best-effort; no-op on non-Windows
    let args = cli::Cli::parse();

    // Shell completions — generate and exit immediately, no elevation or
    // logging needed.
    if let cli::Command::Completions(opts) = &args.command {
        let mut cmd = cli::Cli::command();
        clap_complete::generate(opts.shell, &mut cmd, "dotfiles", &mut std::io::stdout());
        return ExitCode::SUCCESS;
    }

    // Log viewing is read-only: do not initialize the tracing subscriber or
    // create a new log file just to display an existing log.
    if let cli::Command::Log(_) = &args.command {
        return match commands::log::run(args.verbose) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                drop(writeln!(std::io::stderr().lock(), "{e:#}"));
                ExitCode::FAILURE
            }
        };
    }

    let command_name = match &args.command {
        cli::Command::Install(_) => "install",
        cli::Command::Update(_) => "update",
        cli::Command::Uninstall(_) => "uninstall",
        cli::Command::Test(_) => "test",
        cli::Command::Log(_) => "log",
        cli::Command::Version | cli::Command::Completions(_) => "version",
    };
    logging::init_subscriber(args.verbose, command_name);
    let mut raw_log = logging::Logger::new(command_name);
    raw_log.set_verbose(args.verbose);
    let log = std::sync::Arc::new(raw_log);

    // Auto-elevate on Windows for install/uninstall when not in dry-run mode
    #[cfg(windows)]
    {
        let needs_elevation = matches!(
            &args.command,
            cli::Command::Install(_) | cli::Command::Update(_) | cli::Command::Uninstall(_)
        ) && !args.global.dry_run;
        if needs_elevation
            && !elevation::is_elevated()
            && let Err(e) = elevation::elevate_and_exit(&exec::SystemExecutor, &*log)
        {
            log.error(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    }

    // Set up cooperative cancellation so Ctrl-C lets in-flight operations
    // finish cleanly instead of terminating the process immediately.
    let token = engine::CancellationToken::new();
    let handler_token = token.clone();
    let handler_log = std::sync::Arc::clone(&log);
    if ctrlc::set_handler(move || {
        handler_token.cancel();
        handler_log.warn("interrupt received - finishing in-flight operations");
    })
    .is_err()
    {
        // Non-fatal: we just lose graceful shutdown support.
        log.warn("failed to register signal handler");
    }

    let result = match args.command {
        cli::Command::Install(opts) => commands::install::run(&args.global, &opts, &log, &token),
        cli::Command::Update(opts) => commands::update::run(&args.global, &opts, &log, &token),
        cli::Command::Uninstall(opts) => {
            commands::uninstall::run(&args.global, &opts, &log, &token)
        }
        cli::Command::Test(opts) => commands::test::run(&args.global, &opts, &log, &token),
        cli::Command::Version => {
            commands::version::run();
            return ExitCode::SUCCESS;
        }
        // Completions are handled above; this arm is unreachable but kept
        // because the `unreachable!` macro is denied by the lint configuration.
        cli::Command::Log(_) | cli::Command::Completions(_) => return ExitCode::SUCCESS,
    };

    if let Err(e) = result {
        log.error(&format!("{e:#}"));
        log.error("Run 'dotfiles log' for details.");
        elevation::wait_if_elevated();
        return ExitCode::FAILURE;
    }

    elevation::wait_if_elevated();
    ExitCode::SUCCESS
}
