use super::*;
use clap::error::ErrorKind;

fn display_output(args: &[&str], expected_kind: ErrorKind) -> String {
    let error = Cli::try_parse_from(args.iter().copied())
        .expect_err("display arguments should stop normal parsing");
    assert_eq!(error.kind(), expected_kind);
    error.to_string()
}

#[path = "cli_arguments.rs"]
mod arguments;
#[path = "cli_help.rs"]
mod help;
#[path = "cli_options.rs"]
mod options;
