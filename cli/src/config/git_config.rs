//! Git configuration loading.
use serde::Deserialize;

use super::ValidationWarning;
use super::config_section;

/// A git config key-value pair to apply globally.
#[derive(Debug, Clone, Deserialize)]
pub struct GitSetting {
    /// Config key (e.g. `"core.autocrlf"`).
    pub key: String,
    /// Desired value (e.g. `"false"`).
    pub value: String,
}

config_section!(field: "settings", ty: GitSetting);

/// Validate git config entries and return any warnings.
#[must_use]
pub fn validate(settings: &[GitSetting]) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    Validator::new("git-config.toml")
        .check_each(
            settings,
            |setting| &setting.key,
            |setting| {
                vec![
                    check(setting.key.trim().is_empty(), "config key is empty"),
                    check(setting.value.trim().is_empty(), "config value is empty"),
                    check(
                        !setting.key.contains('.'),
                        "config key should contain a section separator (e.g. 'core.autocrlf')",
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
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_windows_settings() {
        let (_dir, path) = write_temp_toml(
            r#"[windows]
settings = [
  { key = "core.autocrlf", value = "false" },
  { key = "core.symlinks", value = "true" },
]
"#,
        );
        let settings = load(&path, &[Category::Windows]).unwrap();
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].key, "core.autocrlf");
        assert_eq!(settings[0].value, "false");
    }

    #[test]
    fn load_excludes_unmatched_category() {
        let (_dir, path) = write_temp_toml(
            r#"[windows]
settings = [{ key = "core.autocrlf", value = "false" }]
"#,
        );
        let settings = load(&path, &[Category::Base, Category::Linux]).unwrap();
        assert!(settings.is_empty());
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    // ------------------------------------------------------------------
    // validate
    // ------------------------------------------------------------------

    #[test]
    fn validate_valid_setting_produces_no_warnings() {
        let settings = vec![GitSetting {
            key: "core.autocrlf".to_string(),
            value: "false".to_string(),
        }];
        assert!(validate(&settings).is_empty());
    }

    #[test]
    fn validate_detects_empty_key() {
        let settings = vec![GitSetting {
            key: "  ".to_string(),
            value: "false".to_string(),
        }];
        let warnings = validate(&settings);
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.message.contains("key is empty")));
    }

    #[test]
    fn validate_detects_empty_value() {
        let settings = vec![GitSetting {
            key: "core.autocrlf".to_string(),
            value: "  ".to_string(),
        }];
        let warnings = validate(&settings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("value is empty"));
    }

    #[test]
    fn validate_detects_missing_section_separator() {
        let settings = vec![GitSetting {
            key: "autocrlf".to_string(),
            value: "false".to_string(),
        }];
        let warnings = validate(&settings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("section separator"));
    }

    #[test]
    fn validate_empty_settings_produces_no_warnings() {
        assert!(validate(&[]).is_empty());
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nsettings = [");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nsettings = [{ key = 123, value = \"ok\" }]\n");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "integer key should fail deserialization");
    }
}
