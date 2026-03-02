//! Dotfiles management engine binary entry point.
use clap::Parser;
use dotfiles_cli::{cli, commands, logging};

fn main() {
    enable_ansi_support::enable_ansi_support().ok(); // best-effort; no-op on non-Windows
    let args = cli::Cli::parse();

    // Auto-elevate on Windows for install/uninstall when not in dry-run mode
    #[cfg(windows)]
    {
        let needs_elevation = matches!(
            &args.command,
            cli::Command::Install(_) | cli::Command::Uninstall(_)
        ) && !args.global.dry_run;
        if needs_elevation
            && !dotfiles_cli::elevation::is_elevated()
            && let Err(e) = dotfiles_cli::elevation::elevate_and_exit()
        {
            eprintln!("\x1b[31mError: {e}\x1b[0m");
            std::process::exit(1);
        }
    }

    let command_name = match &args.command {
        cli::Command::Install(_) => "install",
        cli::Command::Uninstall(_) => "uninstall",
        cli::Command::Test(_) => "test",
        cli::Command::Version => "version",
    };
    logging::init_subscriber(args.verbose, command_name);
    let log = std::sync::Arc::new(logging::Logger::new(command_name));

    let result = match args.command {
        cli::Command::Install(opts) => commands::install::run(&args.global, &opts, &log),
        cli::Command::Uninstall(opts) => commands::uninstall::run(&args.global, &opts, &log),
        cli::Command::Test(opts) => commands::test::run(&args.global, &opts, &log),
        cli::Command::Version => {
            commands::version::run();
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("\x1b[31mError: {e}\x1b[0m");
        dotfiles_cli::elevation::wait_if_elevated();
        std::process::exit(1);
    }

    dotfiles_cli::elevation::wait_if_elevated();
}
