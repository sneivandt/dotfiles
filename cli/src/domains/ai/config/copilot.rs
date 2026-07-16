//! Copilot CLI settings loading.
use serde::Deserialize;

use crate::infra::config::Diagnostic;
use crate::infra::config::config_section;

/// A single Copilot settings key to converge inside a JSON settings document.
///
/// The `key` is a dot-separated path into the JSON document (for example
/// `"model"` or `"footer.showBranch"`), and `value` is the desired value
/// expressed in TOML.  TOML scalars, arrays, and inline tables are converted to
/// their JSON equivalents via [`CopilotSetting::json_value`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopilotSetting {
    /// Dot-separated key path (e.g. `"model"`, `"footer.showBranch"`).
    pub key: String,
    /// Desired value, expressed in TOML and converted to JSON on demand.
    pub value: toml::Value,
}

impl CopilotSetting {
    /// Convert the TOML value into its JSON equivalent.
    ///
    /// The conversion is total: TOML datetimes are rendered as their RFC 3339
    /// string form, and floats that cannot be represented as JSON numbers
    /// (`NaN`, infinities) become `null`.
    #[must_use]
    pub fn json_value(&self) -> serde_json::Value {
        toml_to_json(&self.value)
    }
}

/// Recursively convert a [`toml::Value`] into a [`serde_json::Value`].
fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match value {
        toml::Value::String(s) => Json::String(s.clone()),
        toml::Value::Integer(i) => Json::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        toml::Value::Boolean(b) => Json::Bool(*b),
        toml::Value::Datetime(dt) => Json::String(dt.to_string()),
        toml::Value::Array(items) => Json::Array(items.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Json::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

config_section!(field: "settings", ty: CopilotSetting);

/// Validate Copilot settings entries and return any warnings.
#[must_use]
pub fn validate(settings: &[CopilotSetting]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check};

    Validator::new(COPILOT_TOML)
        .check_each(
            settings,
            |setting| &setting.key,
            |setting| {
                [
                    check(
                        setting.key.trim().is_empty(),
                        "copilot.empty-key",
                        "settings key is empty",
                    ),
                    check(
                        setting.key.split('.').any(str::is_empty),
                        "copilot.key-empty-segment",
                        "settings key has an empty path segment (e.g. 'a..b')",
                    ),
                ]
            },
        )
        .finish()
}

/// TOML filename that backs this config section.
pub(crate) const COPILOT_TOML: &str = "copilot.toml";

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
    use crate::infra::config::test_helpers::write_temp_toml;
    use crate::infra::config::test_load_missing_returns_empty;

    #[test]
    fn load_base_settings() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
settings = [
  { key = "model", value = "claude-opus-4.8" },
  { key = "beep", value = false },
  { key = "footer.showBranch", value = true },
]
"#,
        );
        let settings = load(&path, &[Category::Base]).unwrap();
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].key, "model");
        assert_eq!(
            settings[0].json_value(),
            serde_json::Value::String("claude-opus-4.8".to_string())
        );
        assert_eq!(settings[1].json_value(), serde_json::Value::Bool(false));
        assert_eq!(settings[2].json_value(), serde_json::Value::Bool(true));
    }

    #[test]
    fn json_value_converts_nested_table() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
settings = [
  { key = "footer", value = { showBranch = true, showQuota = false } },
]
"#,
        );
        let settings = load(&path, &[Category::Base]).unwrap();
        let json = settings[0].json_value();
        assert_eq!(json["showBranch"], serde_json::Value::Bool(true));
        assert_eq!(json["showQuota"], serde_json::Value::Bool(false));
    }

    #[test]
    fn load_excludes_unmatched_category() {
        let (_dir, path) = write_temp_toml(
            r#"[desktop]
settings = [{ key = "model", value = "x" }]
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
        let settings = vec![CopilotSetting {
            key: "model".to_string(),
            value: toml::Value::String("claude-opus-4.8".to_string()),
        }];
        assert!(validate(&settings).is_empty());
    }

    #[test]
    fn validate_detects_empty_key() {
        let settings = vec![CopilotSetting {
            key: "  ".to_string(),
            value: toml::Value::Boolean(true),
        }];
        let warnings = validate(&settings);
        assert!(warnings.iter().any(|w| w.message.contains("key is empty")));
    }

    #[test]
    fn validate_detects_empty_path_segment() {
        let settings = vec![CopilotSetting {
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
            "[base]\nsettings = [{ key = \"model\", value = \"x\", typo = \"y\" }]\n",
        );
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "unknown field 'typo' should error");
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nsettings = [");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }
}
