//! Thin binary wrapper around the dotfiles CLI library entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    dotfiles_cli::run()
}
