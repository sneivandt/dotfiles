//! Agent harness settings loading.
use serde::Deserialize;

use crate::infra::config::Diagnostic;
use crate::infra::config::config_section;

crate::infra::config::diagnostic_codes! {
    "agent-settings";
    AGENT_SETTINGS_EMPTY_KEY => "empty-key",
    AGENT_SETTINGS_KEY_EMPTY_SEGMENT => "key-empty-segment",
    AGENT_SETTINGS_CONFLICTING_VALUES => "conflicting-values",
    AGENT_SETTINGS_NONFINITE_VALUE => "nonfinite-value",
}

/// An agent harness with a supported user settings document.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentHarness {
    /// GitHub Copilot CLI.
    Copilot,
    /// `OpenAI` Codex CLI, IDE extension, and desktop app agent.
    Codex,
}

impl AgentHarness {
    /// Stable configuration name for the harness.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Copilot => "copilot",
            Self::Codex => "codex",
        }
    }
}

/// A single agent settings key to converge in one harness's settings document.
///
/// The `key` is a dot-separated path into the JSON document (for example
/// `"footer.showBranch"`) or TOML document (for example
/// `"model_reasoning_effort"` or `"tui.theme"`). The value is expressed in
/// TOML and converted to JSON when the target uses JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSetting {
    /// Harness whose user settings document owns this key.
    pub target: AgentHarness,
    /// Dot-separated key path (e.g. `"model"`, `"footer.showBranch"`).
    pub key: String,
    /// Desired value, expressed in TOML and converted to JSON on demand.
    pub value: toml::Value,
}

config_section!(field: "settings", ty: AgentSetting);

/// Find settings that cannot converge to a single value in their target document.
pub(crate) fn validate_conflicts(settings: &[AgentSetting]) -> Vec<Diagnostic> {
    let mut diagnostics = crate::infra::config::validation::Validator::new(AGENT_SETTINGS_TOML)
        .check_conflicts(
            settings,
            AGENT_SETTINGS_CONFLICTING_VALUES,
            |setting| format!("{}:{}", setting.target.name(), setting.key),
            |setting| setting.value.clone(),
            |_| None,
        )
        .finish();
    for (index, setting) in settings.iter().enumerate() {
        for (previous_index, previous) in settings.iter().take(index).enumerate() {
            if setting.target == previous.target
                && (conflicting_descendant(previous, setting)
                    || conflicting_descendant(setting, previous))
            {
                diagnostics.push(Diagnostic::error(
                    AGENT_SETTINGS_TOML,
                    format!("{}:{}", setting.target.name(), setting.key),
                    AGENT_SETTINGS_CONFLICTING_VALUES,
                    format!(
                        "conflicting parent/child settings at entry {} ({}) and entry {} ({}); keep compatible desired values",
                        previous_index.saturating_add(1), previous.key,
                        index.saturating_add(1), setting.key,
                    ),
                ));
            }
        }
    }
    diagnostics
}

fn conflicting_descendant(parent: &AgentSetting, child: &AgentSetting) -> bool {
    let Some(suffix) = child
        .key
        .strip_prefix(&parent.key)
        .and_then(|key| key.strip_prefix('.'))
    else {
        return false;
    };
    suffix.split('.').try_fold(&parent.value, |value, segment| {
        value.as_table()?.get(segment)
    }) != Some(&child.value)
}

/// JSON cannot represent nonfinite numbers, and TOML NaN cannot compare equal
/// to itself during convergence. Check nested values before any document writes.
pub(crate) fn value_is_finite(value: &toml::Value) -> bool {
    match value {
        toml::Value::Float(value) => value.is_finite(),
        toml::Value::Array(values) => values.iter().all(value_is_finite),
        toml::Value::Table(values) => values.values().all(value_is_finite),
        toml::Value::String(_)
        | toml::Value::Integer(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_) => true,
    }
}

/// Validate agent settings entries and return semantic diagnostics.
#[must_use]
pub fn validate(settings: &[AgentSetting]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check, check_error};

    let mut diagnostics = Validator::new(AGENT_SETTINGS_TOML)
        .check_each(
            settings,
            |setting| &setting.key,
            |setting| {
                [
                    check_error(
                        !value_is_finite(&setting.value),
                        AGENT_SETTINGS_NONFINITE_VALUE,
                        "settings value contains NaN or infinity; use finite numbers",
                    ),
                    check(
                        setting.key.trim().is_empty(),
                        AGENT_SETTINGS_EMPTY_KEY,
                        "settings key is empty",
                    ),
                    check(
                        setting.key.split('.').any(str::is_empty),
                        AGENT_SETTINGS_KEY_EMPTY_SEGMENT,
                        "settings key has an empty path segment (e.g. 'a..b')",
                    ),
                ]
            },
        )
        .finish();
    diagnostics.extend(validate_conflicts(settings));
    diagnostics
}

/// TOML filename that backs this config section.
pub(crate) const AGENT_SETTINGS_TOML: &str = "agent-settings.toml";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::write_temp_toml;
    use crate::infra::config::test_load_missing_returns_empty;

    #[test]
    fn load_base_settings() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
settings = [
  { target = "copilot", key = "model", value = "claude-opus-4.8" },
  { target = "copilot", key = "beep", value = false },
  { target = "codex", key = "model_reasoning_effort", value = "high" },
]
"#,
        );
        let settings = load(&path, &[Category::Base]).unwrap();
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].target, AgentHarness::Copilot);
        assert_eq!(settings[0].key, "model");
        assert_eq!(
            settings[0].value,
            toml::Value::String("claude-opus-4.8".to_string())
        );
        assert_eq!(settings[1].value, toml::Value::Boolean(false));
        assert_eq!(settings[2].target, AgentHarness::Codex);
        assert_eq!(settings[2].value, toml::Value::String("high".to_string()));
    }

    #[test]
    fn load_preserves_nested_table_value() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
settings = [
  { target = "copilot", key = "footer", value = { showBranch = true, showQuota = false } },
]
"#,
        );
        let settings = load(&path, &[Category::Base]).unwrap();
        let table = settings[0].value.as_table().unwrap();
        assert_eq!(table["showBranch"], toml::Value::Boolean(true));
        assert_eq!(table["showQuota"], toml::Value::Boolean(false));
    }

    #[test]
    fn load_excludes_unmatched_category() {
        let (_dir, path) = write_temp_toml(
            r#"[desktop]
settings = [{ target = "codex", key = "model", value = "x" }]
"#,
        );
        let settings = load(&path, &[Category::Base, Category::Linux]).unwrap();
        assert!(settings.is_empty());
    }

    test_load_missing_returns_empty!(load);

    // ------------------------------------------------------------------
    // validate
    // ------------------------------------------------------------------

    #[test]
    fn validate_valid_setting_produces_no_warnings() {
        let settings = vec![AgentSetting {
            target: AgentHarness::Copilot,
            key: "model".to_string(),
            value: toml::Value::String("claude-opus-4.8".to_string()),
        }];
        assert!(validate(&settings).is_empty());
    }

    #[test]
    fn validate_detects_empty_key() {
        let settings = vec![AgentSetting {
            target: AgentHarness::Copilot,
            key: "  ".to_string(),
            value: toml::Value::Boolean(true),
        }];
        let warnings = validate(&settings);
        assert!(warnings.iter().any(|w| w.message.contains("key is empty")));
    }

    #[test]
    fn validate_detects_empty_path_segment() {
        let settings = vec![AgentSetting {
            target: AgentHarness::Codex,
            key: "footer..showBranch".to_string(),
            value: toml::Value::Boolean(true),
        }];
        let warnings = validate(&settings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("empty path segment"));
    }

    #[test]
    fn validate_empty_settings_produces_no_warnings() {
        assert!(validate(&[]).is_empty());
    }

    #[test]
    fn conflicting_values_are_scoped_to_the_harness_and_exact_key() {
        for (target, key, value, conflicts) in [
            (AgentHarness::Copilot, "model", "other", true),
            (AgentHarness::Copilot, "model", "current", false),
            (AgentHarness::Codex, "model", "other", false),
            (AgentHarness::Copilot, "Model", "other", false),
        ] {
            let settings = [
                AgentSetting {
                    target: AgentHarness::Copilot,
                    key: "model".to_string(),
                    value: toml::Value::String("current".to_string()),
                },
                AgentSetting {
                    target,
                    key: key.to_string(),
                    value: toml::Value::String(value.to_string()),
                },
            ];
            let diagnostics = validate(&settings);
            assert_eq!(
                diagnostics.len(),
                usize::from(conflicts),
                "{target:?}:{key}"
            );
            if conflicts {
                let diagnostic = &diagnostics[0];
                assert_eq!(diagnostic.code, AGENT_SETTINGS_CONFLICTING_VALUES);
                assert_eq!(diagnostic.severity, crate::infra::config::Severity::Error);
                assert_eq!(diagnostic.source, AGENT_SETTINGS_TOML);
                assert_eq!(diagnostic.item, "copilot:model");
                assert!(diagnostic.message.contains("entry 1"));
                assert!(diagnostic.message.contains("entry 2"));
            }
        }
    }

    #[test]
    fn load_returns_error_on_unknown_field_in_entry() {
        let (_dir, path) = write_temp_toml(
            "[base]\nsettings = [{ target = \"copilot\", key = \"model\", value = \"x\", typo = \"y\" }]\n",
        );
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "unknown field 'typo' should error");
    }

    #[test]
    fn load_returns_error_on_unknown_target() {
        let (_dir, path) = write_temp_toml(
            "[base]\nsettings = [{ target = \"other\", key = \"model\", value = \"x\" }]\n",
        );
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "unknown target should error");
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nsettings = [");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn parent_child_conflicts_are_checked_in_both_orders_without_exposing_values() {
        let footer_value: toml::Value = toml::from_str::<toml::Table>("showBranch = true\n")
            .unwrap()
            .into();
        for (parent_key, parent_value, child_key, child_value, conflict) in [
            (
                "footer",
                footer_value.clone(),
                "footer.showBranch",
                toml::Value::Boolean(false),
                true,
            ),
            (
                "footer",
                footer_value.clone(),
                "footer.showBranch",
                toml::Value::Boolean(true),
                false,
            ),
            (
                "footer",
                footer_value.clone(),
                "footer.showQuota",
                toml::Value::Boolean(true),
                true,
            ),
            (
                "footer",
                toml::Value::String("private-fixture".into()),
                "footer.showBranch",
                toml::Value::Boolean(true),
                true,
            ),
            (
                "footer",
                toml::from_str::<toml::Table>("[nested]\nshowBranch = true")
                    .unwrap()
                    .into(),
                "footer.nested.showBranch",
                toml::Value::Boolean(false),
                true,
            ),
            (
                "footer",
                footer_value,
                "footerOther.showBranch",
                toml::Value::Boolean(false),
                false,
            ),
        ] {
            for reverse in [false, true] {
                let mut entries = vec![
                    AgentSetting {
                        target: AgentHarness::Copilot,
                        key: parent_key.into(),
                        value: parent_value.clone(),
                    },
                    AgentSetting {
                        target: AgentHarness::Copilot,
                        key: child_key.into(),
                        value: child_value.clone(),
                    },
                ];
                if reverse {
                    entries.reverse();
                }
                let diagnostics = validate_conflicts(&entries);
                assert_eq!(
                    diagnostics.len(),
                    usize::from(conflict),
                    "{parent_key}/{child_key}, reverse={reverse}"
                );
                if conflict {
                    let diagnostic = &diagnostics[0];
                    assert_eq!(diagnostic.code, AGENT_SETTINGS_CONFLICTING_VALUES);
                    assert_eq!(diagnostic.source, AGENT_SETTINGS_TOML);
                    assert_eq!(diagnostic.severity, crate::infra::config::Severity::Error);
                    assert!(diagnostic.message.contains("entry 1"));
                    assert!(diagnostic.message.contains("entry 2"));
                    assert!(!diagnostic.message.contains("private-fixture"));
                }
                entries[0].target = AgentHarness::Codex;
                assert!(validate_conflicts(&entries).is_empty());
            }
        }
    }

    #[test]
    fn nonfinite_numbers_are_rejected_recursively_for_both_harnesses() {
        for target in [AgentHarness::Copilot, AgentHarness::Codex] {
            for raw in [
                "nan",
                "inf",
                "-inf",
                "[1, nan]",
                "{ nested = [0, { value = inf }] }",
            ] {
                let value = toml::from_str::<toml::Table>(&format!("value = {raw}\n"))
                    .unwrap()
                    .remove("value")
                    .unwrap();
                let entries = [AgentSetting {
                    target,
                    key: "fixture".into(),
                    value,
                }];
                let diagnostics = validate(&entries);
                assert_eq!(diagnostics.len(), 1, "{target:?}: {raw}");
                let diagnostic = &diagnostics[0];
                assert_eq!(diagnostic.code, AGENT_SETTINGS_NONFINITE_VALUE);
                assert_eq!(diagnostic.severity, crate::infra::config::Severity::Error);
                assert_eq!(diagnostic.source, AGENT_SETTINGS_TOML);
                assert_eq!(diagnostic.item, "fixture");
            }
        }
        for raw in ["1.25", "[1, -2.5]", "{ nested = [true, 0.0] }"] {
            let value = toml::from_str::<toml::Table>(&format!("value = {raw}\n"))
                .unwrap()
                .remove("value")
                .unwrap();
            assert!(value_is_finite(&value));
        }
    }
}
