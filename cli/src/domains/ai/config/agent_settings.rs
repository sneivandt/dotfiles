//! Agent harness settings loading.
use serde::Deserialize;

use crate::infra::config::Diagnostic;
use crate::infra::config::DiagnosticCode;
use crate::infra::config::config_section;

/// Diagnostic code: `agent-settings.empty-key`.
const AGENT_SETTINGS_EMPTY_KEY: DiagnosticCode = DiagnosticCode::new("agent-settings", "empty-key");
/// Diagnostic code: `agent-settings.key-empty-segment`.
const AGENT_SETTINGS_KEY_EMPTY_SEGMENT: DiagnosticCode =
    DiagnosticCode::new("agent-settings", "key-empty-segment");

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

/// Validate agent settings entries and return any warnings.
#[must_use]
pub fn validate(settings: &[AgentSetting]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check};

    Validator::new(AGENT_SETTINGS_TOML)
        .check_each(
            settings,
            |setting| &setting.key,
            |setting| {
                [
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
        .finish()
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
}
