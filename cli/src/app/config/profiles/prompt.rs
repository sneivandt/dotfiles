//! Interactive profile selection.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::definitions::ProfileDef;
use super::definitions::load_definitions;

/// Interactively prompt the user to select a profile.
///
/// # Errors
///
/// Returns an error if profiles cannot be loaded or user input cannot be read.
pub fn prompt_interactive(conf_dir: &Path) -> Result<String> {
    let definitions = load_definitions(&conf_dir.join("profiles.toml"))?;
    prompt_interactive_with_defs(&definitions)
}

pub(super) fn prompt_interactive_with_defs(
    definitions: &HashMap<String, ProfileDef>,
) -> Result<String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    prompt_interactive_with_io(definitions, &mut stdin.lock(), &mut stdout.lock())
}

fn prompt_interactive_with_io(
    definitions: &HashMap<String, ProfileDef>,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<String> {
    let mut options: Vec<(&str, Option<&str>)> = definitions
        .iter()
        .map(|(name, definition)| (name.as_str(), definition.description.as_deref()))
        .collect();
    options.sort_by_key(|(name, _)| *name);

    if options.is_empty() {
        bail!("no compatible profiles found");
    }

    writeln!(output, "\nSelect a profile:")?;
    for (index, (name, description)) in options.iter().enumerate() {
        if let Some(description) = description {
            writeln!(
                output,
                "  \x1b[1m{}\x1b[0m) {name} \u{2014} {description}",
                index.saturating_add(1)
            )?;
        } else {
            writeln!(
                output,
                "  \x1b[1m{}\x1b[0m) {name}",
                index.saturating_add(1)
            )?;
        }
    }
    write!(output, "\nProfile [1-{}]: ", options.len())?;
    output.flush().context("flushing stdout")?;

    let mut selection = String::new();
    input
        .read_line(&mut selection)
        .context("reading profile selection")?;

    let choice: usize = selection
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn prompt(definitions: &HashMap<String, ProfileDef>, input: &str) -> (Result<String>, String) {
        let mut input = Cursor::new(input.as_bytes());
        let mut output = Vec::new();
        let result = prompt_interactive_with_io(definitions, &mut input, &mut output);
        let output = String::from_utf8(output).expect("prompt output should be UTF-8");
        (result, output)
    }

    #[test]
    fn options_are_sorted_and_the_selected_profile_is_returned() {
        let definitions = super::super::definitions::default_definitions();

        let (result, output) = prompt(&definitions, "2\n");

        assert_eq!(result.expect("selection should succeed"), "desktop");
        let base = output.find(") base").expect("base option");
        let desktop = output.find(") desktop").expect("desktop option");
        assert!(base < desktop, "profile options should be sorted by name");
        assert!(output.contains("Profile [1-2]: "));
        assert!(output.contains("Core shell environment, no desktop GUI"));
    }

    #[test]
    fn empty_profile_set_is_rejected_before_prompting() {
        let (result, output) = prompt(&HashMap::new(), "1\n");

        assert!(
            result
                .expect_err("empty definitions should fail")
                .to_string()
                .contains("no compatible profiles found")
        );
        assert!(
            output.is_empty(),
            "an impossible prompt should not be shown"
        );
    }

    #[test]
    fn malformed_eof_and_out_of_range_selections_are_rejected() {
        let definitions = super::super::definitions::default_definitions();
        for (input, expected) in [
            ("invalid\n", "invalid selection"),
            ("", "invalid selection"),
            ("0\n", "selection out of range"),
            ("3\n", "selection out of range"),
        ] {
            let (result, _output) = prompt(&definitions, input);
            let error = result.expect_err("selection should fail").to_string();
            assert!(
                error.contains(expected),
                "input {input:?} should report {expected:?}, got {error:?}"
            );
        }
    }
}
