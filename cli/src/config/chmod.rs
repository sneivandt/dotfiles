//! Chmod entry configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

/// A file permission directive.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used on unix only (tasks/chmod.rs)
pub struct ChmodEntry {
    /// Permission mode (e.g., "600", "755").
    pub mode: String,
    /// Relative path under $HOME.
    pub path: String,
}

/// TOML section containing chmod entries.
#[derive(Debug, Deserialize)]
struct ChmodSection {
    permissions: Vec<ChmodEntry>,
}

impl toml_loader::ConfigSection for ChmodSection {
    type Entry = ChmodEntry;
    type Item = ChmodEntry;
    const MATCH_MODE: MatchMode = MatchMode::All;

    fn extract(self) -> Vec<ChmodEntry> {
        self.permissions
    }

    fn map(entry: ChmodEntry) -> ChmodEntry {
        entry
    }
}

/// Load chmod entries from chmod.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<ChmodEntry>> {
    toml_loader::load_section::<ChmodSection>(path, active_categories)
}

/// Minimum length for octal mode strings.
const OCTAL_MODE_MIN_LEN: usize = 3;

/// Maximum length for octal mode strings.
const OCTAL_MODE_MAX_LEN: usize = 4;

/// Validates an octal mode string (e.g., "644", "0755").
///
/// Returns `Some(error_message)` if the mode is invalid, or `None` if valid.
fn validate_octal_mode(mode: &str) -> Option<String> {
    if !mode.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!(
            "invalid octal mode '{mode}': must contain only digits"
        ));
    }

    if mode.len() < OCTAL_MODE_MIN_LEN || mode.len() > OCTAL_MODE_MAX_LEN {
        return Some(format!(
            "invalid mode length '{mode}': must be {OCTAL_MODE_MIN_LEN} or {OCTAL_MODE_MAX_LEN} digits"
        ));
    }

    if let Some(c) = mode.chars().find(|&c| c > '7') {
        return Some(format!("invalid octal digit '{c}' in mode '{mode}'"));
    }

    None
}

/// Validate chmod entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[ChmodEntry],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    let mut v = Validator::new("chmod.toml");
    v.warn_if(
        !entries.is_empty() && !platform.supports_chmod(),
        "chmod entries",
        "chmod entries defined but platform does not support chmod",
    );
    v.check_each(
        entries,
        |e| &e.path,
        |e| {
            vec![
                validate_octal_mode(&e.mode),
                check(
                    Path::new(&e.path).is_absolute() || e.path.starts_with('/'),
                    "path should be relative to $HOME directory",
                ),
            ]
        },
    )
    .finish()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn parse_chmod_entry() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/git/ai-pr.sh" },
]
"#,
        );
        let entries = load(&path, &[Category::Base]).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mode, "600");
        assert_eq!(entries[0].path, "ssh/config");
        assert_eq!(entries[1].mode, "755");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn validate_detects_invalid_mode() {
        use crate::platform::{Os, Platform};

        let entries = vec![ChmodEntry {
            mode: "999".to_string(),
            path: ".ssh/config".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("invalid octal digit"));
    }

    #[test]
    fn validate_detects_invalid_mode_length() {
        use crate::platform::{Os, Platform};

        let entries = vec![ChmodEntry {
            mode: "12".to_string(),
            path: ".ssh/config".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("must be 3 or 4 digits"));
    }

    #[test]
    fn validate_octal_mode_accepts_valid_modes() {
        assert_eq!(validate_octal_mode("644"), None);
        assert_eq!(validate_octal_mode("755"), None);
        assert_eq!(validate_octal_mode("0644"), None);
        assert_eq!(validate_octal_mode("0755"), None);
        assert_eq!(validate_octal_mode("600"), None);
        assert_eq!(validate_octal_mode("777"), None);
    }

    #[test]
    fn validate_octal_mode_rejects_non_digits() {
        let result = validate_octal_mode("abc");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must contain only digits"));
    }

    #[test]
    fn validate_octal_mode_rejects_invalid_length() {
        let result = validate_octal_mode("12");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must be 3 or 4 digits"));

        let result = validate_octal_mode("12345");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must be 3 or 4 digits"));
    }

    #[test]
    fn validate_octal_mode_rejects_invalid_octal_digits() {
        let result = validate_octal_mode("888");
        assert!(result.is_some());
        assert!(result.unwrap().contains("invalid octal digit '8'"));

        let result = validate_octal_mode("799");
        assert!(result.is_some());
        assert!(result.unwrap().contains("invalid octal digit '9'"));
    }
}
