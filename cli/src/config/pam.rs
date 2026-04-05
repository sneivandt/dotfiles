//! PAM configuration loading.
use serde::Deserialize;

use super::ValidationWarning;
use super::config_section;

/// A PAM service entry requiring a standard authentication configuration.
///
/// Each entry names a PAM service (e.g. `"hyprlock"`) that will have
/// a configuration file written to `/etc/pam.d/<name>` with standard
/// `system-auth` includes.
#[derive(Debug, Clone, Deserialize)]
pub struct PamEntry {
    /// Service name (written to `/etc/pam.d/<name>`).
    pub name: String,
}

config_section! {
    field: "entries",
    entry: PamEntryRaw,
    item: PamEntry,
    map: |entry| match entry {
        PamEntryRaw::Simple(name) | PamEntryRaw::Table { name } => PamEntry { name },
    },
}

/// Raw deserialization type — accepts both `"hyprlock"` and `{ name = "hyprlock" }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PamEntryRaw {
    /// Bare string form: `"hyprlock"`.
    Simple(String),
    /// Table form: `{ name = "hyprlock" }`.
    Table {
        /// Service name.
        name: String,
    },
}

/// Validate PAM entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[PamEntry],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    let mut v = Validator::new("pam.toml");
    v.warn_if(
        !entries.is_empty() && !platform.is_linux(),
        "pam entries",
        "PAM entries defined but platform is not Linux",
    );
    v.check_each(
        entries,
        |e| &e.name,
        |e| {
            vec![
                check(e.name.trim().is_empty(), "service name is empty"),
                check(e.name.contains('/'), "service name must not contain '/'"),
                check(e.name.contains('.'), "service name must not contain '.'"),
            ]
        },
    )
    .finish()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn parse_simple_string_entry() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
entries = [
    "hyprlock",
]
"#,
        );
        let entries = load(&path, &[Category::Base]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "hyprlock");
    }

    #[test]
    fn parse_table_entry() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
entries = [
    { name = "hyprlock" },
]
"#,
        );
        let entries = load(&path, &[Category::Base]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "hyprlock");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nentries = [");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn validate_detects_empty_name() {
        use crate::platform::{Os, Platform};

        let entries = vec![PamEntry {
            name: String::new(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("empty"));
    }

    #[test]
    fn validate_detects_slash_in_name() {
        use crate::platform::{Os, Platform};

        let entries = vec![PamEntry {
            name: "../shadow".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert!(
            warnings.iter().any(|w| w.message.contains("'/'")),
            "expected slash warning, got: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_on_non_linux() {
        use crate::platform::{Os, Platform};

        let entries = vec![PamEntry {
            name: "hyprlock".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("not Linux"));
    }
}
