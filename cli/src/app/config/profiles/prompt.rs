//! Interactive profile selection.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::definitions::{ProfileDef, load_definitions};

/// Interactively prompt the user to select a profile.
///
/// # Errors
///
/// Returns an error if profiles cannot be loaded or user input cannot be read.
#[cfg(any(test, feature = "internal-api", doctest))]
pub fn prompt_interactive(conf_dir: &Path) -> Result<String> {
    let definitions = load_definitions(&conf_dir.join("profiles.toml"))?;
    prompt_interactive_with_defs(&definitions)
}

#[allow(clippy::print_stdout, reason = "intentional user-facing output")]
pub(super) fn prompt_interactive_with_defs(
    definitions: &HashMap<String, ProfileDef>,
) -> Result<String> {
    let mut options: Vec<(&str, Option<&str>)> = definitions
        .iter()
        .map(|(name, definition)| (name.as_str(), definition.description.as_deref()))
        .collect();
    options.sort_by_key(|(name, _)| *name);

    if options.is_empty() {
        bail!("no compatible profiles found");
    }

    println!("\nSelect a profile:");
    for (index, (name, description)) in options.iter().enumerate() {
        if let Some(description) = description {
            println!(
                "  \x1b[1m{}\x1b[0m) {name} \u{2014} {description}",
                index.saturating_add(1)
            );
        } else {
            println!("  \x1b[1m{}\x1b[0m) {name}", index.saturating_add(1));
        }
    }
    print!("\nProfile [1-{}]: ", options.len());
    io::stdout().flush().context("flushing stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("reading profile selection")?;

    let choice: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid selection"))?;

    if choice == 0 || choice > options.len() {
        bail!("selection out of range");
    }

    options
        .get(choice.saturating_sub(1))
        .map(|(name, _)| (*name).to_string())
        .context("selection out of range")
}
