//! Systemd unit configuration loading.
use serde::Deserialize;

use crate::infra::config::Diagnostic;
use crate::infra::config::StringOrTable;
use crate::infra::config::config_section;

crate::infra::config::diagnostic_codes! {
    "systemd";
    SYSTEMD_EMPTY_NAME => "empty-name",
    SYSTEMD_INVALID_EXTENSION => "invalid-extension",
    SYSTEMD_INVALID_SCOPE => "invalid-scope",
    SYSTEMD_PLATFORM_UNSUPPORTED => "platform-unsupported",
    SYSTEMD_INVALID_NAME => "invalid-name",
    SYSTEMD_CONFLICTING_VALUES => "conflicting-values",
}

/// A systemd unit and its desired enablement state.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemdUnit {
    /// Unit name including extension (e.g., `"clean-home-tmp.timer"`).
    pub name: String,
    /// Systemd scope.
    pub scope: UnitScope,
    /// Whether the unit should be enabled and started.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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

const fn default_enabled() -> bool {
    true
}

/// A single entry in a units section, either a plain name string or a
/// structured entry with explicit options.
type UnitEntry = StringOrTable<SystemdUnit>;

config_section! {
    field: "units",
    entry: UnitEntry,
    item: SystemdUnit,
    map: |entry| match entry {
        StringOrTable::Bare(name) => SystemdUnit {
            name,
            scope: UnitScope::User,
            enabled: true,
        },
        StringOrTable::Table(unit) => unit,
    },
}

/// Valid systemd unit file extensions.
const VALID_UNIT_EXTENSIONS: &[&str] = &[
    ".service",
    ".timer",
    ".socket",
    ".target",
    ".path",
    ".mount",
    ".automount",
    ".swap",
];

/// Unit names are filesystem components and command arguments, never paths.
pub(crate) fn safe_unit_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 || name.starts_with(['.', '-']) {
        return false;
    }
    let mut bytes = name.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            if bytes.next() != Some(b'x')
                || !bytes.next().is_some_and(|digit| digit.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|digit| digit.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !byte.is_ascii_alphanumeric() && !b":_.@-".contains(&byte) {
            return false;
        }
    }
    true
}

/// Contradictory declarations cannot converge, even when processed sequentially.
pub(crate) fn validate_conflicts(units: &[SystemdUnit]) -> Vec<Diagnostic> {
    crate::infra::config::validation::Validator::new(SYSTEMD_UNITS_TOML)
        .check_conflicts(
            units,
            SYSTEMD_CONFLICTING_VALUES,
            |unit| format!("{:?}:{}", unit.scope, unit.name),
            |unit| unit.enabled,
            |_| None,
        )
        .finish()
}

/// Validate systemd unit entries and return any warnings.
#[must_use]
pub fn validate(
    units: &[SystemdUnit],
    platform: crate::infra::platform::Platform,
) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check, check_error};

    let mut diagnostics = Validator::new(SYSTEMD_UNITS_TOML)
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
                check_error(!safe_unit_name(&u.name), SYSTEMD_INVALID_NAME,
                    "unit name must be a single safe systemd name, not a path or option"),
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
        .finish();
    diagnostics.extend(validate_conflicts(units));
    diagnostics
}

/// TOML filename that backs this config section.
pub(crate) const SYSTEMD_UNITS_TOML: &str = "systemd-units.toml";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::{assert_load_rejects, write_temp_toml};
    use crate::infra::config::test_load_missing_returns_empty;

    #[test]
    fn safe_names_reject_paths_options_controls_and_malformed_escapes() {
        for name in [
            "../../audit.service",
            "/audit.service",
            "./audit.service",
            "--root.service",
            ".hidden.service",
            "audit name.service",
            "audit\n.service",
            "audit\0.service",
            r"a\b.service",
            r"a\x.service",
            r"a\x2.service",
            r"a\xgg.service",
            r"a\x2g.service",
            r"a\X22.service",
            "é.service",
            "",
        ] {
            assert!(!safe_unit_name(name), "{name:?}");
        }
        assert!(!safe_unit_name(&"a".repeat(256)));
        assert!(safe_unit_name(&"a".repeat(255)));
        for name in ["audit.service", "a-b_c:1.2.service", r"a\x2d\xAF.mount"] {
            assert!(safe_unit_name(name), "{name:?}");
        }
        for separator in ['-', '_', ':', '.', '@'] {
            assert!(safe_unit_name(&format!("audit{separator}one.service")));
        }
        let units = [SystemdUnit {
            name: "../audit.service".into(),
            scope: UnitScope::User,
            enabled: true,
        }];
        let diagnostics = validate(
            &units,
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
        );
        let diagnostic = diagnostics
            .iter()
            .find(|d| d.code == SYSTEMD_INVALID_NAME)
            .unwrap();
        assert_eq!(diagnostic.severity, crate::infra::config::Severity::Error);
        assert_eq!(diagnostic.source, SYSTEMD_UNITS_TOML);
        assert_eq!(diagnostic.item, "../audit.service");
    }

    #[test]
    fn conflicts_are_scoped_by_unit_and_scope_and_identical_values_are_harmless() {
        let first = SystemdUnit {
            name: "audit.service".into(),
            scope: UnitScope::User,
            enabled: true,
        };
        for (name, scope, enabled, count) in [
            ("audit.service", UnitScope::User, true, 0),
            ("audit.service", UnitScope::User, false, 1),
            ("audit.service", UnitScope::System, false, 0),
            ("other.service", UnitScope::User, false, 0),
        ] {
            let diagnostics = validate_conflicts(&[
                first.clone(),
                SystemdUnit {
                    name: name.into(),
                    scope,
                    enabled,
                },
            ]);
            assert_eq!(diagnostics.len(), count);
            if let Some(diagnostic) = diagnostics.first() {
                assert_eq!(diagnostic.code, SYSTEMD_CONFLICTING_VALUES);
                assert_eq!(diagnostic.severity, crate::infra::config::Severity::Error);
                assert!(diagnostic.message.contains("entry 1"));
                assert!(diagnostic.message.contains("entry 2"));
            }
        }
    }

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
        assert!(units[0].enabled);
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
        assert!(units[0].enabled);
    }

    #[test]
    fn load_disabled_unit() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = [{ name = "dhcpcd.service", scope = "system", enabled = false }]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].name, "dhcpcd.service");
        assert_eq!(units[0].scope, UnitScope::System);
        assert!(!units[0].enabled);
    }

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_invalid_extension() {
        use crate::infra::platform::{Os, Platform};

        let units = vec![SystemdUnit {
            name: "myunit".to_string(),
            scope: UnitScope::User,
            enabled: true,
        }];
        let warnings = validate(&units, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid systemd extension"));
    }

    #[test]
    fn validate_accepts_supported_unit_types_and_rejects_invalid_suffixes() {
        use crate::infra::config::Severity;
        use crate::infra::platform::{Os, Platform};

        let platform = Platform::new(Os::Linux, false);
        for (name, valid) in [
            ("example.service", true),
            ("example.timer", true),
            ("example.socket", true),
            ("example.target", true),
            ("example.path", true),
            ("data.mount", true),
            ("data.automount", true),
            ("swapfile.swap", true),
            ("data.AUTOMOUNT", false),
            ("swapfile.swapp", false),
            ("data.automount.bak", false),
            ("example.unknown", false),
        ] {
            let units = [SystemdUnit {
                name: name.to_string(),
                scope: UnitScope::System,
                enabled: true,
            }];
            let diagnostics = validate(&units, platform);
            if valid {
                assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
            } else {
                assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics:?}");
                let diagnostic = &diagnostics[0];
                assert_eq!(diagnostic.code, SYSTEMD_INVALID_EXTENSION, "{name}");
                assert_eq!(diagnostic.severity, Severity::Warning, "{name}");
                assert_eq!(diagnostic.source, SYSTEMD_UNITS_TOML, "{name}");
                assert_eq!(diagnostic.item, name);
                assert!(diagnostic.message.contains("valid systemd extension"));
            }
        }
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
            enabled: true,
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
            enabled: true,
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
