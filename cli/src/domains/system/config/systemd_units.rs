//! Systemd unit configuration loading.
use serde::Deserialize;

use crate::infra::config::Diagnostic;
use crate::infra::config::DiagnosticCode;
use crate::infra::config::StringOrTable;
use crate::infra::config::config_section;

/// Diagnostic code: `systemd.empty-name`.
const SYSTEMD_EMPTY_NAME: DiagnosticCode = DiagnosticCode::new("systemd", "empty-name");
/// Diagnostic code: `systemd.invalid-extension`.
const SYSTEMD_INVALID_EXTENSION: DiagnosticCode =
    DiagnosticCode::new("systemd", "invalid-extension");
/// Diagnostic code: `systemd.invalid-scope`.
const SYSTEMD_INVALID_SCOPE: DiagnosticCode = DiagnosticCode::new("systemd", "invalid-scope");
/// Diagnostic code: `systemd.platform-unsupported`.
const SYSTEMD_PLATFORM_UNSUPPORTED: DiagnosticCode =
    DiagnosticCode::new("systemd", "platform-unsupported");

/// A systemd unit to enable.
#[derive(Debug, Clone)]
pub struct SystemdUnit {
    /// Unit name including extension (e.g., `"clean-home-tmp.timer"`).
    pub name: String,
    /// Systemd scope.
    pub scope: UnitScope,
}

/// Scope in which a systemd unit is managed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UnitScope {
    /// Manage the unit for the current user.
    #[default]
    User,
    /// Manage the system-wide unit.
    System,
    /// Preserve an unsupported configured value for aggregated validation.
    Invalid(String),
}

impl<'de> Deserialize<'de> for UnitScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as Deserialize>::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "user" => Self::User,
            "system" => Self::System,
            _ => Self::Invalid(value),
        })
    }
}

/// The explicit table form of a unit entry: `{ name, scope }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitWithScope {
    /// Unit name including extension.
    name: String,
    /// Systemd scope for the unit.
    scope: UnitScope,
}

/// A single entry in a units section — either a plain name string or a
/// structured `{ name, scope }` pair for an explicit scope override.
type UnitEntry = StringOrTable<UnitWithScope>;

config_section! {
    field: "units",
    entry: UnitEntry,
    item: SystemdUnit,
    map: |entry| match entry {
        StringOrTable::Bare(name) => SystemdUnit {
            name,
            scope: UnitScope::User,
        },
        StringOrTable::Table(UnitWithScope { name, scope }) => SystemdUnit { name, scope },
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
    platform: crate::infra::platform::Platform,
) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check};

    Validator::new(SYSTEMD_UNITS_TOML)
        .warn_if(
            !units.is_empty() && !platform.supports_systemd(),
            SYSTEMD_PLATFORM_UNSUPPORTED,
            "systemd units",
            "systemd units defined but platform does not support systemd",
        )
        .check_each(units, |u| &u.name, |u| {
            // Note: systemd unit extensions are case-sensitive on Linux
            #[allow(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "extensions are ASCII-only"
            )]
            let has_valid_ext = VALID_UNIT_EXTENSIONS
                .iter()
                .any(|ext| u.name.ends_with(ext));
            [
                check(u.name.trim().is_empty(), SYSTEMD_EMPTY_NAME, "unit name is empty"),
                check(
                    matches!(u.scope, UnitScope::Invalid(_)),
                    SYSTEMD_INVALID_SCOPE,
                    "unit scope should be 'user' or 'system'",
                ),
                check(
                    !has_valid_ext,
                    SYSTEMD_INVALID_EXTENSION,
                    "unit name should end with a valid systemd extension (.service, .timer, .socket, etc.)",
                ),
            ]
        })
        .finish()
}

/// TOML filename that backs this config section.
pub(crate) const SYSTEMD_UNITS_TOML: &str = "systemd-units.toml";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::{assert_load_rejects, write_temp_toml};
    use crate::infra::config::test_load_missing_returns_empty;

    #[test]
    fn unknown_key_in_unit_table_is_rejected() {
        assert_load_rejects(
            load,
            r#"[base]
units = [{ name = "dunst.service", scop = "system" }]
"#,
            "scop",
        );
    }

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
        assert_eq!(units[0].scope, UnitScope::User);
    }

    #[test]
    fn load_plain_string_defaults_to_user_scope() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = ["clean-home-tmp.timer"]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].scope, UnitScope::User);
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
        assert_eq!(units[0].scope, UnitScope::System);
    }

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_invalid_extension() {
        use crate::infra::platform::{Os, Platform};

        let units = vec![SystemdUnit {
            name: "myunit".to_string(),
            scope: UnitScope::User,
        }];
        let warnings = validate(&units, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid systemd extension"));
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nunits = [\"ssh.service\"");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nunits = 42\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
    }

    #[test]
    fn validate_detects_empty_unit_name() {
        use crate::infra::platform::{Os, Platform};

        let units = vec![SystemdUnit {
            name: "  ".to_string(),
            scope: UnitScope::User,
        }];
        let warnings = validate(&units, Platform::new(Os::Linux, false));
        assert!(
            warnings.iter().any(|w| w.message.contains("empty")),
            "should warn about empty unit name: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_systemd_on_non_linux() {
        use crate::infra::platform::{Os, Platform};

        let units = vec![SystemdUnit {
            name: "test.service".to_string(),
            scope: UnitScope::User,
        }];
        let warnings = validate(&units, Platform::new(Os::Windows, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("does not support systemd")),
            "should warn about systemd on non-Linux: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_on_invalid_scope() {
        use crate::infra::platform::{Os, Platform};

        let (_dir, path) = write_temp_toml(
            "[base]\nunits = [{ name = \"example.service\", scope = \"global\" }]\n",
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].scope, UnitScope::Invalid("global".to_string()));
        let warnings = validate(&units, Platform::new(Os::Linux, false));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code.to_string() == "systemd.invalid-scope")
        );
    }
}
