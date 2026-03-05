//! Git configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

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

impl toml_loader::ConfigSection for GitConfigSection {
    type Entry = GitSetting;
    type Item = GitSetting;
    const MATCH_MODE: MatchMode = MatchMode::All;

    fn extract(self) -> Vec<GitSetting> {
        self.settings
    }

    fn map(entry: GitSetting) -> GitSetting {
        entry
    }
}

/// Load git settings from git-config.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<GitSetting>> {
    toml_loader::load_section::<GitConfigSection>(path, active_categories)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
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
