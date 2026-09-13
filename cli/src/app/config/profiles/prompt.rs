//! Interactive profile selection.

use std::io::{self, BufRead, Write};

use anyhow::{Context as _, Result, bail};

use super::{ProfileInfo, available};

/// Interactively prompt the user to select a profile.
///
/// # Errors
///
/// Returns an error if user input cannot be read.
pub fn prompt_interactive() -> Result<String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    prompt_interactive_with_io(available(), &mut stdin.lock(), &mut stdout.lock())
}

fn prompt_interactive_with_io(
    profiles: &[ProfileInfo],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<String> {
    writeln!(output, "\nSelect a profile:")?;
    for (index, profile) in profiles.iter().enumerate() {
        writeln!(
            output,
            "  \x1b[1m{}\x1b[0m) {} \u{2014} {}",
            index.saturating_add(1),
            profile.name,
            profile.description,
        )?;
    }
    write!(output, "\nProfile [1-{}]: ", profiles.len())?;
    output.flush().context("flushing stdout")?;

    let mut selection = String::new();
    input
        .read_line(&mut selection)
        .context("reading profile selection")?;

    let choice: usize = selection
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid selection"))?;

    if choice == 0 || choice > profiles.len() {
        bail!("selection out of range");
    }

    profiles
        .get(choice.saturating_sub(1))
        .map(|profile| profile.name.to_string())
        .context("selection out of range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn prompt(input: &str) -> (Result<String>, String) {
        let mut input = Cursor::new(input.as_bytes());
        let mut output = Vec::new();
        let result = prompt_interactive_with_io(available(), &mut input, &mut output);
        let output = String::from_utf8(output).expect("prompt output should be UTF-8");
        (result, output)
    }

    #[test]
    fn built_in_options_and_the_selected_profile_are_returned() {
        let (result, output) = prompt("2\n");

        assert_eq!(result.expect("selection should succeed"), "desktop");
        let base = output.find(") base").expect("base option");
        let desktop = output.find(") desktop").expect("desktop option");
        assert!(base < desktop, "base should be listed before desktop");
        assert!(output.contains("Profile [1-2]: "));
        assert!(output.contains("Core shell environment, no desktop GUI"));
    }

    #[test]
    fn malformed_eof_and_out_of_range_selections_are_rejected() {
        for (input, expected) in [
            ("invalid\n", "invalid selection"),
            ("", "invalid selection"),
            ("0\n", "selection out of range"),
            ("3\n", "selection out of range"),
        ] {
            let (result, _output) = prompt(input);
            let error = result.expect_err("selection should fail").to_string();
            assert!(
                error.contains(expected),
                "input {input:?} should report {expected:?}, got {error:?}"
            );
        }
    }
}
