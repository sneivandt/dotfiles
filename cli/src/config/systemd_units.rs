//! Systemd unit configuration loading.
use serde::Deserialize;

use super::ValidationWarning;
use super::config_section;

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

config_section! {
    field: "units",
    entry: UnitEntry,
    item: SystemdUnit,
    map: |entry| match entry {
        UnitEntry::Simple(name) => SystemdUnit {
            name,
            scope: "user".to_string(),
        },
        UnitEntry::WithScope { name, scope } => SystemdUnit { name, scope },
    },
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
    use super::helpers::validation::{Validator, check};

    let mut v = Validator::new("systemd-units.toml");
    v.warn_if(
        !units.is_empty() && !platform.supports_systemd(),
        "systemd units",
        "systemd units defined but platform does not support systemd",
    );
    v.check_each(units, |u| &u.name, |u| {
        // Note: systemd unit extensions are case-sensitive on Linux
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        let has_valid_ext = VALID_UNIT_EXTENSIONS
            .iter()
            .any(|ext| u.name.ends_with(ext));
        vec![
            check(u.name.trim().is_empty(), "unit name is empty"),
            check(
                !has_valid_ext,
                "unit name should end with a valid systemd extension (.service, .timer, .socket, etc.)",
            ),
        ]
    })
    .finish()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
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
