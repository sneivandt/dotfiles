//! Git configuration loading.
use serde::Deserialize;
use std::path::Path;

use crate::infra::config::Diagnostic;
use crate::infra::config::DiagnosticCode;
use crate::infra::config::category_matcher::{Category, matches, parse_section_key};
use crate::infra::config::toml_loader;
use crate::infra::config::validation::Validator;

/// Diagnostic code: `git.empty-key`.
const GIT_EMPTY_KEY: DiagnosticCode = DiagnosticCode::new("git", "empty-key");
/// Diagnostic code: `git.empty-value`.
const GIT_EMPTY_VALUE: DiagnosticCode = DiagnosticCode::new("git", "empty-value");
/// Diagnostic code: `git.key-missing-section`.
const GIT_KEY_MISSING_SECTION: DiagnosticCode = DiagnosticCode::new("git", "key-missing-section");
/// Diagnostic code: `git.conflicting-values`.
const GIT_CONFLICTING_VALUES: DiagnosticCode = DiagnosticCode::new("git", "conflicting-values");

/// A git config key-value pair to apply globally.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSetting {
    /// Config key (e.g. `"core.autocrlf"`).
    pub key: String,
    /// Desired value (e.g. `"false"`).
    pub value: String,
    /// File, section, and entry position of the declaration.
    #[serde(skip)]
    pub(crate) origin: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    settings: Vec<GitSetting>,
}

/// Load active Git settings while retaining their declaration locations.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> anyhow::Result<Vec<GitSetting>> {
    let sections = toml_loader::load_section_items(path, |section: Section| section.settings)?;
    Ok(sections
        .into_iter()
        .filter(|(section, _)| matches(&parse_section_key(section), active_categories))
        .flat_map(|(section, settings)| {
            settings
                .into_iter()
                .enumerate()
                .map(move |(index, mut setting)| {
                    setting.origin = Some(format!(
                        "{} [{section}] settings entry {}",
                        path.display(),
                        index.saturating_add(1)
                    ));
                    setting
                })
        })
        .collect())
}

fn target_key(key: &str) -> String {
    let Some((section, rest)) = key.split_once('.') else {
        return key.to_ascii_lowercase();
    };
    // Git sections and variable names are case-insensitive; subsections are not.
    let section = section.to_ascii_lowercase();
    rest.rsplit_once('.').map_or_else(
        || format!("{section}.{}", rest.to_ascii_lowercase()),
        |(subsection, variable)| {
            format!("{section}.{subsection}.{}", variable.to_ascii_lowercase())
        },
    )
}

/// Find contradictory declarations among the active main and overlay settings.
pub(crate) fn validate_conflicts(settings: &[GitSetting]) -> Vec<Diagnostic> {
    Validator::new(GIT_CONFIG_TOML)
        .check_conflicts(
            settings,
            GIT_CONFLICTING_VALUES,
            |setting| target_key(&setting.key),
            |setting| setting.value.clone(),
            |setting| setting.origin.as_deref(),
        )
        .finish()
}

/// Validate git config entries and return any warnings.
#[must_use]
pub fn validate(settings: &[GitSetting]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::check;

    let mut diagnostics = Validator::new(GIT_CONFIG_TOML)
        .check_each(
            settings,
            |setting| &setting.key,
            |setting| {
                [
                    check(
                        setting.key.trim().is_empty(),
                        GIT_EMPTY_KEY,
                        "config key is empty",
                    ),
                    check(
                        setting.value.trim().is_empty(),
                        GIT_EMPTY_VALUE,
                        "config value is empty",
                    ),
                    check(
                        !setting.key.contains('.'),
                        GIT_KEY_MISSING_SECTION,
                        "config key should contain a section separator (e.g. 'core.autocrlf')",
                    ),
                ]
            },
        )
        .finish();
    diagnostics.extend(validate_conflicts(settings));
    diagnostics
}

/// TOML filename that backs this config section.
pub(crate) const GIT_CONFIG_TOML: &str = "git-config.toml";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::write_temp_toml;
    use crate::infra::config::test_load_missing_returns_empty;

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

    test_load_missing_returns_empty!(load);

    // ------------------------------------------------------------------
    // validate
    // ------------------------------------------------------------------

    #[test]
    fn validate_valid_setting_produces_no_warnings() {
        let settings = vec![GitSetting {
            key: "core.autocrlf".to_string(),
            value: "false".to_string(),
            origin: None,
        }];
        assert!(validate(&settings).is_empty());
    }

    #[test]
    fn validate_detects_empty_key() {
        let settings = vec![GitSetting {
            key: "  ".to_string(),
            value: "false".to_string(),
            origin: None,
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
            origin: None,
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
            origin: None,
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
    fn conflicting_values_respect_git_key_identity() {
        let cases = [
            (
                "different values",
                "core.editor",
                "core.editor",
                "vim",
                "nano",
                true,
            ),
            (
                "identical values",
                "core.editor",
                "CORE.EDITOR",
                "vim",
                "vim",
                false,
            ),
            (
                "section and variable case",
                "core.editor",
                "CORE.EDITOR",
                "vim",
                "nano",
                true,
            ),
            (
                "same subsection",
                "remote.Origin.url",
                "REMOTE.Origin.URL",
                "a",
                "b",
                true,
            ),
            (
                "subsection case",
                "remote.Origin.url",
                "remote.origin.url",
                "a",
                "b",
                false,
            ),
            (
                "dotted subsection",
                "remote.a.B.url",
                "REMOTE.a.B.URL",
                "a",
                "b",
                true,
            ),
            (
                "distinct variables",
                "core.editor",
                "core.pager",
                "vim",
                "less",
                false,
            ),
            (
                "literal values",
                "core.editor",
                "core.editor",
                "Vim",
                "vim",
                true,
            ),
        ];
        for (name, first_key, second_key, first_value, second_value, conflict) in cases {
            let settings = [
                GitSetting {
                    key: first_key.to_string(),
                    value: first_value.to_string(),
                    origin: None,
                },
                GitSetting {
                    key: second_key.to_string(),
                    value: second_value.to_string(),
                    origin: None,
                },
            ];
            let diagnostics = validate(&settings);
            assert_eq!(
                diagnostics.len(),
                usize::from(conflict),
                "{name}: {diagnostics:?}"
            );
            if conflict {
                assert_eq!(diagnostics[0].code, GIT_CONFLICTING_VALUES, "{name}");
                assert_eq!(
                    diagnostics[0].severity,
                    crate::infra::config::Severity::Error,
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn conflicting_values_report_both_declaration_locations() {
        let (_dir, path) = write_temp_toml(
            r#"
[base]
settings = [
  { key = "core.editor", value = "vim" },
  { key = "core.editor", value = "nano" },
]
[desktop]
settings = [{ key = "core.editor", value = "code" }]
"#,
        );
        let settings = load(&path, &[Category::Base]).expect("load active settings");
        let diagnostics = validate_conflicts(&settings);
        assert_eq!(
            settings.len(),
            2,
            "inactive desktop declarations must not participate"
        );
        assert_eq!(diagnostics.len(), 1);
        let message = &diagnostics[0].message;
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("[base] settings entry 1"), "{message}");
        assert!(message.contains("[base] settings entry 2"), "{message}");
    }

    #[test]
    fn conflicting_values_across_active_categories_are_rejected() {
        let (_dir, path) = write_temp_toml(
            "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n\
             [desktop]\nsettings = [{ key = \"core.editor\", value = \"nano\" }]\n",
        );
        let settings = load(&path, &[Category::Base, Category::Desktop]).unwrap();
        assert_eq!(validate_conflicts(&settings).len(), 1);
    }

    #[test]
    fn declaration_origin_cannot_be_supplied_by_configuration() {
        let (_dir, path) = write_temp_toml(
            "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\", origin = \"fake\" }]\n",
        );
        assert!(
            load(&path, &[Category::Base]).is_err(),
            "provenance is loader-owned"
        );
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

    #[test]
    fn load_returns_error_on_unknown_field_in_entry() {
        let (_dir, path) = write_temp_toml(
            "[base]\nsettings = [{ key = \"core.autocrlf\", value = \"false\", typo = \"x\" }]\n",
        );
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "unknown field 'typo' in GitSetting should return an error"
        );
    }

    #[test]
    fn load_returns_error_on_unknown_section_field() {
        // Wrong field name in the section: "setting" instead of "settings".
        let (_dir, path) =
            write_temp_toml("[base]\nsetting = [{ key = \"core.autocrlf\", value = \"false\" }]\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "unknown section field 'setting' should return an error"
        );
    }
}
