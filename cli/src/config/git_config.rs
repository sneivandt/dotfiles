//! Git configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

/// A git config key-value pair to apply globally.
#[derive(Debug, Clone, Deserialize)]
pub struct GitSetting {
    /// Config key (e.g. `"core.autocrlf"`).
    pub key: String,
    /// Desired value (e.g. `"false"`).
    pub value: String,
}

/// TOML section containing git settings.
#[derive(Debug, Deserialize)]
struct GitConfigSection {
    settings: Vec<GitSetting>,
}

/// Load git settings from git-config.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<GitSetting>> {
    let config: HashMap<String, GitConfigSection> = toml_loader::load_config(path)?;

    let items: Vec<(String, Vec<GitSetting>)> =
        config.into_iter().map(|(k, v)| (k, v.settings)).collect();

    Ok(toml_loader::filter_by_categories(
        items,
        active_categories,
        MatchMode::All,
    ))
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
}
