//! Interactive profile selection.

use std::io::{self, BufRead, Write};

use anyhow::{Context as _, Result};

use super::{ProfileInfo, available};
use crate::infra::logging::{ColorMode, StyleChoice, stdout_style_for};

/// Interactively prompt the user to select a profile.
///
/// # Errors
///
/// Returns an error if user input cannot be read.
pub fn prompt_interactive(color: ColorMode) -> Result<String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    prompt_interactive_with_io(
        available(),
        &mut stdin.lock(),
        &mut stdout.lock(),
        stdout_style_for(color),
    )
}

fn prompt_interactive_with_io(
    profiles: &[ProfileInfo],
    input: &mut impl BufRead,
    output: &mut impl Write,
    style: StyleChoice,
) -> Result<String> {
    writeln!(output, "\nSelect a profile:")?;
    for (index, profile) in profiles.iter().enumerate() {
        writeln!(
            output,
            "  {}) {} \u{2014} {}",
            style.bold(&index.saturating_add(1).to_string()),
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

    choice
        .checked_sub(1)
        .and_then(|index| profiles.get(index))
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
        let result = prompt_interactive_with_io(
            available(),
            &mut input,
            &mut output,
            StyleChoice::auto(false, false),
        );
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

    #[test]
    fn option_numbers_follow_the_injected_terminal_and_color_policy() {
        for (terminal, no_color) in [(false, false), (true, false), (true, true)] {
            let mut input = Cursor::new(b"1\n");
            let mut output = Vec::new();
            assert_eq!(
                prompt_interactive_with_io(
                    available(),
                    &mut input,
                    &mut output,
                    StyleChoice::auto(terminal, no_color)
                )
                .unwrap(),
                "base",
            );
            let text = String::from_utf8(output).unwrap();
            if terminal && !no_color {
                assert!(text.contains("  \x1b[1m1\x1b[0m) base"));
                assert_eq!(text.matches("\x1b[1m").count(), 2);
            } else {
                assert!(text.contains("  1) base"));
                assert!(!text.contains('\x1b'));
            }
            assert!(text.contains("base — Core shell environment, no desktop GUI"));
        }
    }
}
