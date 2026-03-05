//! GitHub Copilot skills configuration loading.

use super::ValidationWarning;
use super::config_section;

/// A GitHub Copilot skill URL.
#[derive(Debug, Clone)]
pub struct CopilotSkill {
    /// GitHub blob or tree URL pointing to the skill directory.
    pub url: String,
}

config_section! {
    field: "skills",
    entry: String,
    item: CopilotSkill,
    map: |url| CopilotSkill { url },
}

/// Validate Copilot skill entries and return any warnings.
#[must_use]
pub fn validate(skills: &[CopilotSkill]) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    Validator::new("copilot-skills.toml")
        .check_each(
            skills,
            |skill| &skill.url,
            |skill| {
                vec![
                    check(skill.url.trim().is_empty(), "skill URL is empty"),
                    check(
                        !skill.url.starts_with("http://") && !skill.url.starts_with("https://"),
                        "skill URL should start with http:// or https://",
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
