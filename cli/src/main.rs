use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod config;
mod exec;
mod logging;
mod platform;
mod tasks;

fn main() -> Result<()> {
    let _ = enable_ansi_support::enable_ansi_support();
    let args = cli::Cli::parse();
    let log = logging::Logger::new(args.verbose);

    match args.command {
        cli::Command::Install(opts) => commands::install::run(&args.global, &opts, &log),
        cli::Command::Uninstall(opts) => commands::uninstall::run(&args.global, &opts, &log),
        cli::Command::Test(opts) => commands::test::run(&args.global, &opts, &log),
        cli::Command::Version => {
            let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
            println!("dotfiles {version}");
            Ok(())
        }
    }
}
