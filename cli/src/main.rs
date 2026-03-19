//! Dotfiles management engine binary entry point.
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use dotfiles_cli::{cli, commands, engine, logging};

fn main() -> ExitCode {
    enable_ansi_support::enable_ansi_support().ok(); // best-effort; no-op on non-Windows
    let args = cli::Cli::parse();

    // Shell completions — generate and exit immediately, no elevation or
    // logging needed.
    if let cli::Command::Completions(opts) = &args.command {
        let mut cmd = cli::Cli::command();
        clap_complete::generate(opts.shell, &mut cmd, "dotfiles", &mut std::io::stdout());
        return ExitCode::SUCCESS;
    }

    // Auto-elevate on Windows for install/uninstall when not in dry-run mode
    #[cfg(windows)]
    {
        let needs_elevation = matches!(
            &args.command,
            cli::Command::Install(_) | cli::Command::Uninstall(_)
        ) && !args.global.dry_run;
        if needs_elevation
            && !dotfiles_cli::elevation::is_elevated()
            && let Err(e) =
                dotfiles_cli::elevation::elevate_and_exit(&dotfiles_cli::exec::SystemExecutor)
        {
            eprintln!("\x1b[31mError: {e}\x1b[0m");
            return ExitCode::FAILURE;
        }
    }

    // Set up cooperative cancellation so Ctrl-C lets in-flight operations
    // finish cleanly instead of terminating the process immediately.
    let token = engine::CancellationToken::new();
    let handler_token = token.clone();
    if ctrlc::set_handler(move || {
        handler_token.cancel();
        eprintln!("\x1b[33minterrupt received — finishing in-flight operations\x1b[0m");
    })
    .is_err()
    {
        // Non-fatal: we just lose graceful shutdown support.
        eprintln!("\x1b[33mWarning: failed to register signal handler\x1b[0m");
    }

    let command_name = match &args.command {
        cli::Command::Install(_) => "install",
        cli::Command::Uninstall(_) => "uninstall",
        cli::Command::Test(_) => "test",
        cli::Command::Version | cli::Command::Completions(_) => "version",
    };
    logging::init_subscriber(args.verbose, command_name);
    let mut log = logging::Logger::new(command_name);
    log.set_verbose(args.verbose);
    let log = std::sync::Arc::new(log);

    let result = match args.command {
        cli::Command::Install(opts) => commands::install::run(&args.global, &opts, &log, &token),
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
        cli::Command::Completions(_) => return ExitCode::SUCCESS,
    };

    if let Err(e) = result {
        eprintln!("\x1b[31mError: {e}\x1b[0m");
        dotfiles_cli::elevation::wait_if_elevated();
        return ExitCode::FAILURE;
    }

    dotfiles_cli::elevation::wait_if_elevated();
    ExitCode::SUCCESS
}
