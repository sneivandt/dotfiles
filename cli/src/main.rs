use clap::Parser;

mod cli;
mod commands;
mod config;
mod exec;
mod logging;
mod operations;
mod platform;
mod resources;
mod tasks;

fn main() {
    enable_ansi_support::enable_ansi_support().ok(); // best-effort; no-op on non-Windows
    let args = cli::Cli::parse();
    let command_name = match &args.command {
        cli::Command::Install(_) => "install",
        cli::Command::Uninstall(_) => "uninstall",
        cli::Command::Test(_) => "test",
        cli::Command::Version => "version",
    };
    let log = std::sync::Arc::new(logging::Logger::new(args.verbose, command_name));

    let result = match args.command {
        cli::Command::Install(opts) => commands::install::run(&args.global, &opts, &log),
        cli::Command::Uninstall(opts) => commands::uninstall::run(&args.global, &opts, &log),
        cli::Command::Test(opts) => commands::test::run(&args.global, &opts, &log),
        cli::Command::Version => {
            let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
            println!("dotfiles {version}");
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("\x1b[31mError: {e}\x1b[0m");
        std::process::exit(1);
    }
}
