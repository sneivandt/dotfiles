//! Chmod entry configuration loading.
use serde::Deserialize;
use std::path::Path;

use crate::domains::files::OctalMode;
use crate::runtime::config_support::Diagnostic;
use crate::runtime::config_support::config_section;

/// A file permission directive.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "used conditionally via cfg")] // fields used on unix only (tasks/chmod.rs)
pub struct ChmodEntry {
    /// Configured permission mode (e.g., `"600"`, `"755"`).
    pub mode: String,
    /// Relative path under $HOME.
    pub path: String,
    parsed_mode: Result<OctalMode, String>,
}

#[derive(Deserialize)]
struct RawChmodEntry {
    mode: String,
    path: String,
}

impl<'de> Deserialize<'de> for ChmodEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawChmodEntry::deserialize(deserializer)?;
        Ok(Self::new(raw.mode, raw.path))
    }
}

impl ChmodEntry {
    /// Create an entry and parse its mode once for validation and resource use.
    #[must_use]
    pub fn new(mode: impl Into<String>, path: impl Into<String>) -> Self {
        let mode = mode.into();
        let parsed_mode = OctalMode::parse(&mode);
        Self {
            mode,
            path: path.into(),
            parsed_mode,
        }
    }

    pub(crate) const fn parsed_mode(&self) -> &Result<OctalMode, String> {
        &self.parsed_mode
    }
}

config_section!(field: "permissions", ty: ChmodEntry);

/// Validate chmod entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[ChmodEntry],
    platform: crate::runtime::platform::Platform,
) -> Vec<Diagnostic> {
    use crate::runtime::config_support::Severity;
    use crate::runtime::config_support::validation::{Validator, check, check_error};

    Validator::new(CHMOD_TOML)
        .warn_if(
            !entries.is_empty() && !platform.supports_chmod(),
            "chmod.platform-unsupported",
            "chmod entries",
            "chmod entries defined but platform does not support chmod",
        )
        .check_each(
            entries,
            |e| &e.path,
            |e| {
                [
                    e.parsed_mode
                        .as_ref()
                        .err()
                        .map(|message| ("chmod.invalid-mode", Severity::Warning, message.clone())),
                    check(
                        Path::new(&e.path).is_absolute() || e.path.starts_with('/'),
                        "chmod.absolute-path",
                        "path should be relative to $HOME directory",
                    ),
                    check_error(
                        Path::new(&e.path)
                            .components()
                            .any(|c| c == std::path::Component::ParentDir),
                        "chmod.parent-in-path",
                        "path must not contain '..' components",
                    ),
                ]
            },
        )
        .finish()
}

/// TOML filename that backs this config section.
pub(crate) const CHMOD_TOML: &str = "chmod.toml";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::runtime::config_support::category_matcher::Category;
    use crate::runtime::config_support::test_helpers::write_temp_toml;
    use crate::runtime::config_support::test_load_missing_returns_empty;

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

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_invalid_mode() {
        use crate::runtime::platform::{Os, Platform};

        let entries = vec![ChmodEntry::new("999", ".ssh/config")];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("invalid octal digit"));
    }

    #[test]
    fn validate_detects_invalid_mode_length() {
        use crate::runtime::platform::{Os, Platform};

        let entries = vec![ChmodEntry::new("12", ".ssh/config")];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("must be 3 or 4 digits"));
    }

    #[test]
    fn validate_detects_path_traversal() {
        use crate::runtime::platform::{Os, Platform};

        let entries = vec![ChmodEntry::new("600", "../../etc/shadow")];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("'..'"));
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\npermissions = [");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\npermissions = \"not-an-array\"\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "string instead of array should return error"
        );
    }
}
