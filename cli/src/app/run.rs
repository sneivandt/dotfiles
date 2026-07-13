//! CLI startup wiring: argument parsing, logging setup, cancellation,
//! elevation handling, and command dispatch.

use std::io::Write as _;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};

#[cfg(windows)]
use crate::runtime::exec;
use crate::runtime::{elevation, error, logging};

use super::{cli, commands};

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

    let Some(command_name) = logged_command_name(&args.command) else {
        return ExitCode::SUCCESS;
    };
    logging::init_subscriber(args.verbose, command_name);
    let mut raw_log = logging::Logger::new(command_name);
    raw_log.set_verbose(args.verbose);
    raw_log.set_dry_run(args.global.dry_run);
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
    let token = crate::engine::CancellationToken::new();
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
        // Completions and log are handled above; these arms are unreachable but
        // kept because the lint configuration denies the `unreachable!` macro.
        cli::Command::Log(_) | cli::Command::Completions(_) => return ExitCode::SUCCESS,
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
    if error.downcast_ref::<error::TaskFailures>().is_none() {
        log.error(&format!("{error:#}"));
    }
    log.always("Run 'dotfiles log' for details.");
}

const fn logged_command_name(command: &cli::Command) -> Option<&'static str> {
    let name = match command {
        cli::Command::Install(_) => "install",
        cli::Command::Update(_) => "update",
        cli::Command::Uninstall(_) => "uninstall",
        cli::Command::Test(_) => "test",
        cli::Command::Version => "version",
        cli::Command::Log(_) | cli::Command::Completions(_) => return None,
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};

    use super::*;

    #[derive(Default)]
    struct CapturingOutput {
        errors: Mutex<Vec<String>>,
        always: Mutex<Vec<String>>,
    }

    impl logging::Output for CapturingOutput {
        fn stage(&self, _msg: &str) {}
        fn info(&self, _msg: &str) {}
        fn debug(&self, _msg: &str) {}
        fn warn(&self, _msg: &str) {}
        fn error(&self, msg: &str) {
            self.errors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(msg.to_string());
        }
        fn dry_run(&self, _msg: &str) {}
        fn always(&self, msg: &str) {
            self.always
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(msg.to_string());
        }
    }

    #[test]
    fn aggregate_task_failure_only_prints_plain_log_hint() {
        let log = CapturingOutput::default();
        let error = anyhow::Error::from(error::TaskFailures::new(2));

        report_failure(&error, &log);

        assert!(
            log.errors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty(),
            "aggregate task failure should not repeat the failed task count"
        );
        assert_eq!(
            *log.always.lock().unwrap_or_else(PoisonError::into_inner),
            ["Run 'dotfiles log' for details."],
            "log hint should use the always-visible plain-text channel"
        );
    }

    #[test]
    fn unexpected_failure_still_prints_error_and_plain_log_hint() {
        let log = CapturingOutput::default();
        let error = anyhow::anyhow!("configuration failed");

        report_failure(&error, &log);

        assert_eq!(
            *log.errors.lock().unwrap_or_else(PoisonError::into_inner),
            ["configuration failed"],
            "unexpected command failures should remain visible as errors"
        );
        assert_eq!(
            *log.always.lock().unwrap_or_else(PoisonError::into_inner),
            ["Run 'dotfiles log' for details."],
            "log hint should use the always-visible plain-text channel"
        );
    }
}
