//! Command: print version information.

/// Print the dotfiles version to stdout.
#[allow(clippy::print_stdout, reason = "intentional user-facing output")]
pub fn run() {
    let version =
        option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
    println!("dotfiles {version}");
}
