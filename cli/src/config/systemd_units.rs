//! Systemd unit configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

/// A systemd unit to enable.
#[derive(Debug, Clone)]
pub struct SystemdUnit {
    /// Unit name including extension (e.g., `"clean-home-tmp.timer"`).
    pub name: String,
    /// Systemd scope: `"user"` or `"system"` (default: `"user"`).
    pub scope: String,
}

/// A single entry in a units section — either a plain name string or a
/// structured `{ name, scope }` pair for an explicit scope override.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum UnitEntry {
    /// Plain string: `"dunst.service"` — defaults to user scope.
    Simple(String),
    /// Structured: `{ name = "foo.service", scope = "system" }`.
    WithScope { name: String, scope: String },
}

/// TOML section containing systemd units.
#[derive(Debug, Deserialize)]
struct SystemdSection {
    units: Vec<UnitEntry>,
}

/// Load systemd units from systemd-units.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<SystemdUnit>> {
    toml_loader::load_filtered(
        path,
        |s: SystemdSection| s.units,
        |entry| match entry {
            UnitEntry::Simple(name) => SystemdUnit {
                name,
                scope: "user".to_string(),
            },
            UnitEntry::WithScope { name, scope } => SystemdUnit { name, scope },
        },
        active_categories,
        MatchMode::All,
    )
}

/// Valid systemd unit file extensions.
const VALID_UNIT_EXTENSIONS: &[&str] = &[
    ".service", ".timer", ".socket", ".target", ".path", ".mount",
];

/// Validate systemd unit entries and return any warnings.
#[must_use]
pub fn validate(
    units: &[SystemdUnit],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    if !units.is_empty() && !platform.supports_systemd() {
        warnings.push(ValidationWarning::new(
            "systemd-units.toml",
            "systemd units",
            "systemd units defined but platform does not support systemd",
        ));
    }

    for unit in units {
        if unit.name.trim().is_empty() {
            warnings.push(ValidationWarning::new(
                "systemd-units.toml",
                &unit.name,
                "unit name is empty",
            ));
        }

        // Note: systemd unit extensions are case-sensitive on Linux
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        if !VALID_UNIT_EXTENSIONS
            .iter()
            .any(|ext| unit.name.ends_with(ext))
        {
            warnings.push(ValidationWarning::new(
                "systemd-units.toml",
                &unit.name,
                "unit name should end with a valid systemd extension (.service, .timer, .socket, etc.)",
            ));
        }
    }

    warnings
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_base_units() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = ["clean-home-tmp.timer"]

["arch-desktop"]
units = ["dunst.service"]
"#,
        );
        let units: Vec<SystemdUnit> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "clean-home-tmp.timer");
        assert_eq!(units[0].scope, "user");
    }

    #[test]
    fn load_plain_string_defaults_to_user_scope() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = ["clean-home-tmp.timer"]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].scope, "user");
    }

    #[test]
    fn load_scope_override() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = [{ name = "some-daemon.service", scope = "system" }]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].name, "some-daemon.service");
        assert_eq!(units[0].scope, "system");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn validate_detects_invalid_extension() {
        use crate::platform::{Os, Platform};

        let units = vec![SystemdUnit {
            name: "myunit".to_string(),
            scope: "user".to_string(),
        }];
        let warnings = validate(&units, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid systemd extension"));
    }
}
