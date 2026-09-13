//! Read-only profile discovery.

use anyhow::Result;

use crate::app::cli::ProfilesOpts;
use crate::app::config::profiles::ProfileInfo;

/// List built-in role profiles without selecting or persisting one.
///
/// # Errors
///
/// Returns an error if output cannot be written.
pub fn run(_opts: &ProfilesOpts) -> Result<()> {
    let profiles = crate::app::config::profiles::available();
    let stdout = std::io::stdout();
    write_profiles(profiles, &mut stdout.lock())
}

fn write_profiles(profiles: &[ProfileInfo], out: &mut dyn std::io::Write) -> Result<()> {
    let name_width = profiles
        .iter()
        .map(|profile| profile.name.len())
        .max()
        .unwrap_or("PROFILE".len())
        .max("PROFILE".len());
    writeln!(out, "{:<name_width$}  DESCRIPTION", "PROFILE")?;
    for profile in profiles {
        writeln!(
            out,
            "{:<name_width$}  {}",
            profile.name, profile.description
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lists_names_and_descriptions() {
        let profiles = vec![
            ProfileInfo {
                name: "base",
                description: "Command-line environment",
            },
            ProfileInfo {
                name: "desktop",
                description: "Graphical workstation",
            },
        ];
        let mut output = Vec::new();

        write_profiles(&profiles, &mut output).expect("profile table");

        let output = String::from_utf8(output).expect("UTF-8 output");
        assert!(output.contains("PROFILE  DESCRIPTION"));
        assert!(output.contains("base     Command-line environment"));
        assert!(output.contains("desktop  Graphical workstation"));
    }
}
