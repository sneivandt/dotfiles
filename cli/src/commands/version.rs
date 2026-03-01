//! Command: print version information.

/// Print the dotfiles version to stdout.
pub fn run() {
    let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    println!("dotfiles {version}");
}
