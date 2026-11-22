//! Git configuration loading.
use serde::Deserialize;
#[cfg(test)]
use std::path::Path;

use crate::infra::config::Diagnostic;
#[cfg(test)]
use crate::infra::config::category_matcher::Category;
use crate::infra::config::toml_loader;
use crate::infra::config::validation::Validator;

crate::infra::config::diagnostic_codes! {
    "git";
    GIT_EMPTY_KEY => "empty-key",
    GIT_EMPTY_VALUE => "empty-value",
    GIT_KEY_MISSING_SECTION => "key-missing-section",
    GIT_CONFLICTING_VALUES => "conflicting-values",
}

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
#[cfg(test)]
fn load(path: &Path, active_categories: &[Category]) -> anyhow::Result<Vec<GitSetting>> {
    Ok(toml_loader::filter_by_categories(
        toml_loader::with_optional_document(path, decode)?,
        active_categories,
    ))
}

/// Decode settings once, retaining origins before deriving category views.
pub(crate) fn decode(
    document: &toml_loader::ConfigDocument<'_>,
) -> anyhow::Result<Vec<(String, Vec<GitSetting>)>> {
    let path = &document.path;
    let sections = document.section_items(|section: Section| section.settings)?;
    Ok(sections
        .into_iter()
        .map(|(section, mut settings)| {
            for (index, setting) in settings.iter_mut().enumerate() {
                setting.origin = Some(format!(
                    "{} [{section}] settings entry {}",
                    path.display(),
                    index.saturating_add(1)
                ));
            }
            (section, settings)
        })
        .collect())
}

pub(crate) fn target_key(key: &str) -> String {
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

/// Find contradictory declarations among active settings in one source.
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
    use crate::infra::config::test_helpers::{
        assert_conflict, assert_mentions, assert_messages, rejects_toml, write_temp_toml,
    };
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

    fn setting(key: &str, value: &str) -> GitSetting {
        GitSetting {
            key: key.into(),
            value: value.into(),
            origin: None,
        }
    }

    #[test]
    fn validate_git_settings() {
        for (entry, count, message) in [
            (setting("  ", "false"), 2, "key is empty"),
            (setting("core.autocrlf", "  "), 1, "value is empty"),
            (setting("autocrlf", "false"), 1, "section separator"),
        ] {
            assert_messages(&validate(&[entry]), Some(count), &[message]);
        }
        assert!(validate(&[setting("core.autocrlf", "false")]).is_empty());
        assert!(validate(&[]).is_empty());
    }

    #[test]
    fn conflicting_values_respect_git_key_identity() {
        for (name, first, second, conflict) in [
            (
                "different values",
                setting("core.editor", "vim"),
                setting("core.editor", "nano"),
                true,
            ),
            (
                "identical values",
                setting("core.editor", "vim"),
                setting("CORE.EDITOR", "vim"),
                false,
            ),
            (
                "section and variable case",
                setting("core.editor", "vim"),
                setting("CORE.EDITOR", "nano"),
                true,
            ),
            (
                "same subsection",
                setting("remote.Origin.url", "a"),
                setting("REMOTE.Origin.URL", "b"),
                true,
            ),
            (
                "subsection case",
                setting("remote.Origin.url", "a"),
                setting("remote.origin.url", "b"),
                false,
            ),
            (
                "dotted subsection",
                setting("remote.a.B.url", "a"),
                setting("REMOTE.a.B.URL", "b"),
                true,
            ),
            (
                "distinct variables",
                setting("core.editor", "vim"),
                setting("core.pager", "less"),
                false,
            ),
            (
                "literal values",
                setting("core.editor", "Vim"),
                setting("core.editor", "vim"),
                true,
            ),
        ] {
            let diagnostics = validate(&[first, second]);
            assert_conflict(name, &diagnostics, conflict, GIT_CONFLICTING_VALUES);
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
        assert_mentions(
            &diagnostics[0].message,
            &[
                &path.display().to_string(),
                "[base] settings entry 1",
                "[base] settings entry 2",
            ],
        );
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

    rejects_toml! {
        "test.toml", |path| load(path, &[Category::Base]);
        declaration_origin_cannot_be_supplied_by_configuration:
            "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\", origin = \"fake\" }]\n";
        load_returns_error_on_malformed_toml: "[base\nsettings = [";
        load_returns_error_on_type_mismatch: "[base]\nsettings = [{ key = 123, value = \"ok\" }]\n";
        load_returns_error_on_unknown_field_in_entry:
            "[base]\nsettings = [{ key = \"core.autocrlf\", value = \"false\", typo = \"x\" }]\n";
        load_returns_error_on_unknown_section_field:
            "[base]\nsetting = [{ key = \"core.autocrlf\", value = \"false\" }]\n";
    }
}
