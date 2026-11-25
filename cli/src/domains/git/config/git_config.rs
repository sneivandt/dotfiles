//! Git configuration loading.
use serde::Deserialize;
use std::collections::BTreeMap;
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
#[derive(Debug, Clone)]
pub struct GitSetting {
    /// Config key (e.g. `"core.autocrlf"`).
    pub key: String,
    /// Desired value (e.g. `"false"`).
    pub value: String,
    /// File, section, and key of the declaration.
    pub(crate) origin: Option<String>,
}

enum GitConfigNode {
    Value(String),
    Table(BTreeMap<String, Self>),
}

impl<'de> Deserialize<'de> for GitConfigNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;

        impl<'de> serde::de::Visitor<'de> for NodeVisitor {
            type Value = GitConfigNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string value or nested table of git settings")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(GitConfigNode::Value(value.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(GitConfigNode::Value(value))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, GitConfigNode>()? {
                    entries.insert(key, value);
                }
                if entries.is_empty() {
                    return Err(serde::de::Error::custom(
                        "nested git config table must contain at least one setting",
                    ));
                }
                Ok(GitConfigNode::Table(entries))
            }
        }

        deserializer.deserialize_any(NodeVisitor)
    }
}

fn flatten_nodes(
    path: &Path,
    section: &str,
    prefix: Option<&str>,
    nodes: BTreeMap<String, GitConfigNode>,
    out: &mut Vec<GitSetting>,
) {
    for (segment, node) in nodes {
        let key = match prefix {
            Some(parent) => format!("{parent}.{segment}"),
            None => segment,
        };
        match node {
            GitConfigNode::Value(value) => {
                let origin = Some(format!("{} [{section}] {key}", path.display()));
                out.push(GitSetting { key, value, origin });
            }
            GitConfigNode::Table(nested) => {
                flatten_nodes(path, section, Some(&key), nested, out);
            }
        }
    }
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
    let path = document.path;
    let sections: BTreeMap<String, BTreeMap<String, GitConfigNode>> = document.deserialize()?;
    Ok(sections
        .into_iter()
        .map(|(section, nodes)| {
            let mut settings = Vec::new();
            flatten_nodes(path, &section, None, nodes, &mut settings);
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
pub(crate) fn validate(settings: &[GitSetting]) -> Vec<Diagnostic> {
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
core.autocrlf = "false"
core.symlinks = "true"
"remote.Origin.url" = "https://example.com/repo.git"
"#,
        );
        let settings = load(&path, &[Category::Windows]).unwrap();
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].key, "core.autocrlf");
        assert_eq!(settings[0].value, "false");
        assert_eq!(settings[1].key, "core.symlinks");
        assert_eq!(settings[1].value, "true");
        assert_eq!(settings[2].key, "remote.Origin.url");
        assert_eq!(settings[2].value, "https://example.com/repo.git");
    }

    #[test]
    fn load_excludes_unmatched_category() {
        let (_dir, path) = write_temp_toml(
            r#"[windows]
core.autocrlf = "false"
"#,
        );
        let settings = load(&path, &[Category::Base, Category::Linux]).unwrap();
        assert!(settings.is_empty());
    }

    #[test]
    fn load_empty_category_section_returns_empty() {
        let (_dir, path) = write_temp_toml("[base]\n");
        let settings = load(&path, &[Category::Base]).unwrap();
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
core.editor = "vim"
CORE.EDITOR = "nano"

[desktop]
core.editor = "code"
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
                "[base] CORE.EDITOR",
                "[base] core.editor",
            ],
        );
    }

    #[test]
    fn conflicting_values_across_active_categories_are_rejected() {
        let (_dir, path) = write_temp_toml(
            "[base]\ncore.editor = \"vim\"\n\
             [desktop]\ncore.editor = \"nano\"\n",
        );
        let settings = load(&path, &[Category::Base, Category::Desktop]).unwrap();
        assert_eq!(validate_conflicts(&settings).len(), 1);
    }

    rejects_toml! {
        "test.toml", |path| load(path, &[Category::Base]);
        load_returns_error_on_malformed_toml: "[base\ncore.editor = \"vim\"";
        load_returns_error_on_integer_value: "[base]\ncore.editor = 123\n";
        load_returns_error_on_boolean_value: "[base]\ncore.symlinks = true\n";
        load_returns_error_on_legacy_settings_array:
            "[base]\nsettings = [{ key = \"core.autocrlf\", value = \"false\" }]\n";
        load_returns_error_on_empty_nested_table: "[base.core]\n";
        load_returns_error_on_empty_inline_table: "[base]\ncore = {}\n";
    }
}
