//! CLI startup wiring: argument parsing, logging setup, cancellation,
//! and command dispatch.
//!
//! Elevation is deliberately *not* handled here. Privilege is planned per task
//! during execution so a run stays unelevated unless a specific task needs
//! otherwise; see `app::commands::execution::prepare_elevation`.

use std::io::Write as _;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};

use crate::infra::{elevation, logging};

use super::{catalog, cli, commands, interrupt};
use crate::infra::logging::OutputExt as _;

/// Run the dotfiles CLI and return the process exit code.
///
/// This is the only supported public entry point for the library crate.  The
/// binary target delegates here so argument parsing, logging setup, graceful
/// cancellation, elevation handling, and command dispatch live in one place.
#[must_use]
pub fn run() -> ExitCode {
    drop(enable_ansi_support::enable_ansi_support()); // best-effort; no-op on non-Windows
    let args = cli::Cli::parse();

    if args.global.elevated_child {
        elevation::mark_elevated_child();
    }

    // Meta commands run standalone and exit before the logging subsystem,
    // elevation, and task engine are initialised. Narrowing to `EngineCommand`
    // here keeps the engine dispatch in `run_engine` total.
    let command = match args.command {
        cli::Command::Completions(opts) => {
            if matches!(opts.shell, clap_complete::Shell::PowerShell) {
                let script = catalog::generate_powershell_completions();
                drop(std::io::stdout().lock().write_all(script.as_bytes()));
            } else {
                let mut cmd = cli::Cli::command();
                clap_complete::generate(opts.shell, &mut cmd, "dotfiles", &mut std::io::stdout());
            }
            return ExitCode::SUCCESS;
        }
        // Log viewing is read-only: do not initialize the tracing subscriber or
        // create a new log file just to display an existing log.
        cli::Command::Log(opts) => {
            return match commands::log::run(&opts, args.verbose) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    drop(writeln!(std::io::stderr().lock(), "{e:#}"));
                    ExitCode::FAILURE
                }
            };
        }
        cli::Command::Install(opts) => cli::EngineCommand::Install(opts),
        cli::Command::Update(opts) => cli::EngineCommand::Update(opts),
        cli::Command::Uninstall(opts) => cli::EngineCommand::Uninstall(opts),
        cli::Command::Test(opts) => cli::EngineCommand::Test(opts),
        cli::Command::Tasks => cli::EngineCommand::Tasks,
    };

    run_engine(&command, &args.global, args.verbose)
}

/// Initialise the runtime and dispatch a command through the task engine.
fn run_engine(command: &cli::EngineCommand, global: &cli::GlobalOpts, verbose: bool) -> ExitCode {
    let mut raw_log = logging::init(verbose, !global.no_symbols, command.name());
    raw_log.set_dry_run(global.dry_run);
    let log = std::sync::Arc::new(raw_log);

    // Set up cooperative cancellation so Ctrl-C lets in-flight operations
    // finish cleanly instead of terminating the process immediately, and
    // escalates to a confirmed force quit when the user asks twice.
    let token = crate::engine::CancellationToken::new();
    interrupt::install(&token, &log);

    let result = match command {
        cli::EngineCommand::Install(opts) => commands::install::run(global, opts, &log, &token),
        cli::EngineCommand::Update(opts) => commands::update::run(global, opts, &log, &token),
        cli::EngineCommand::Uninstall(opts) => commands::uninstall::run(global, opts, &log, &token),
        cli::EngineCommand::Test(opts) => commands::test::run(global, opts, &log, &token),
        cli::EngineCommand::Tasks => commands::tasks::run(global, &log, &token),
    };

    if let Err(e) = result {
        report_failure(&e, &*log);
        elevation::wait_if_elevated();
        return ExitCode::FAILURE;
    }

    elevation::wait_if_elevated();
    ExitCode::SUCCESS
}

fn report_failure(error: &anyhow::Error, log: &dyn logging::Output) {
    if error
        .downcast_ref::<commands::error::TaskFailures>()
        .is_none()
    {
        log.error(format!("{error:#}"));
    }
    log.startup("Run 'dotfiles log' for details.");
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};

    use super::*;

    #[derive(Default)]
    struct CapturingOutput {
        errors: Mutex<Vec<String>>,
        startup: Mutex<Vec<String>>,
    }

    impl logging::Output for CapturingOutput {
        fn emit(&self, kind: logging::MsgKind, msg: std::borrow::Cow<'_, str>) {
            let sink = match kind {
                logging::MsgKind::Error => &self.errors,
                logging::MsgKind::Startup => &self.startup,
                logging::MsgKind::Stage
                | logging::MsgKind::TaskStage
                | logging::MsgKind::Info
                | logging::MsgKind::Debug
                | logging::MsgKind::Trace
                | logging::MsgKind::Always
                | logging::MsgKind::Warn
                | logging::MsgKind::DryRun => return,
            };
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(msg.into_owned());
        }
    }

    #[test]
    fn aggregate_task_failure_only_prints_dim_log_hint() {
        let log = CapturingOutput::default();
        let error = anyhow::Error::from(commands::error::TaskFailures::new(2));

        report_failure(&error, &log);

        assert!(
            log.errors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty(),
            "aggregate task failure should not repeat the failed task count"
        );
        assert_eq!(
            *log.startup.lock().unwrap_or_else(PoisonError::into_inner),
            ["Run 'dotfiles log' for details."],
            "log hint should use the always-visible dim channel"
        );
    }

    #[test]
    fn unexpected_failure_still_prints_error_and_dim_log_hint() {
        let log = CapturingOutput::default();
        let error = anyhow::anyhow!("configuration failed");

        report_failure(&error, &log);

        assert_eq!(
            *log.errors.lock().unwrap_or_else(PoisonError::into_inner),
            ["configuration failed"],
            "unexpected command failures should remain visible as errors"
        );
        assert_eq!(
            *log.startup.lock().unwrap_or_else(PoisonError::into_inner),
            ["Run 'dotfiles log' for details."],
            "log hint should use the always-visible dim channel"
        );
    }
}
