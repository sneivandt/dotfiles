//! Read-only profile discovery.

use anyhow::Result;

use crate::app::cli::ProfilesOpts;
use crate::app::config::profiles::ProfileInfo;

/// List configured role profiles without selecting or persisting one.
///
/// # Errors
///
/// Returns an error if the repository root or profile definitions cannot be
/// read, or output cannot be written.
pub fn run(opts: &ProfilesOpts) -> Result<()> {
    let root = super::runner::resolve_root_path(opts.root.as_deref())?;
    let profiles = crate::app::config::profiles::available(&root.join("conf"))?;
    let stdout = std::io::stdout();
    write_profiles(&profiles, &mut stdout.lock())
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
            profile.name,
            profile.description.as_deref().unwrap_or("")
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
                name: "base".to_string(),
                description: Some("Command-line environment".to_string()),
            },
            ProfileInfo {
                name: "minimal".to_string(),
                description: None,
            },
        ];
        let mut output = Vec::new();

        write_profiles(&profiles, &mut output).expect("profile table");

        let output = String::from_utf8(output).expect("UTF-8 output");
        assert!(output.contains("PROFILE  DESCRIPTION"));
        assert!(output.contains("base     Command-line environment"));
        assert!(output.contains("minimal"));
    }
}
