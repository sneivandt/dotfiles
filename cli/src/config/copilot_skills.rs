//! GitHub Copilot skills configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

/// A GitHub Copilot skill URL.
#[derive(Debug, Clone)]
pub struct CopilotSkill {
    /// GitHub blob or tree URL pointing to the skill directory.
    pub url: String,
}

/// TOML section containing Copilot skill URLs.
#[derive(Debug, Deserialize)]
struct SkillSection {
    skills: Vec<String>,
}

/// Load Copilot skills from copilot-skills.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<CopilotSkill>> {
    let config: HashMap<String, SkillSection> = toml_loader::load_config(path)?;

    let items: Vec<(String, Vec<String>)> =
        config.into_iter().map(|(k, v)| (k, v.skills)).collect();

    let urls: Vec<String> =
        toml_loader::filter_by_categories(items, active_categories, MatchMode::All);

    Ok(urls.into_iter().map(|url| CopilotSkill { url }).collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_base_skills() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
skills = [
  "https://github.com/example/skill1",
  "https://github.com/example/skill2",
]
"#,
        );
        let skills: Vec<CopilotSkill> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills[0].url.starts_with("https://"));
    }

    #[test]
    fn inactive_category_excluded() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
skills = ["https://github.com/example/base-skill"]

[desktop]
skills = ["https://github.com/example/desktop-skill"]
"#,
        );
        let skills: Vec<CopilotSkill> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(skills.len(), 1, "desktop section should not be loaded");
        assert!(skills[0].url.contains("base-skill"));
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
