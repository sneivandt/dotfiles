use super::define_flat_config;

define_flat_config! {
    /// A GitHub Copilot skill URL.
    CopilotSkill { url }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::ini;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn load_base_skills() {
        let (_dir, path) = write_temp_ini(
            "[base]\nhttps://github.com/example/skill1\nhttps://github.com/example/skill2\n",
        );
        let skills: Vec<CopilotSkill> = ini::load_flat(&path, &["base".to_string()]).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills[0].url.starts_with("https://"));
    }

    #[test]
    fn inactive_category_excluded() {
        let (_dir, path) = write_temp_ini(
            "[base]\nhttps://github.com/example/base-skill\n\n[desktop]\nhttps://github.com/example/desktop-skill\n",
        );
        let skills: Vec<CopilotSkill> = ini::load_flat(&path, &["base".to_string()]).unwrap();
        assert_eq!(skills.len(), 1, "desktop section should not be loaded");
        assert!(skills[0].url.contains("base-skill"));
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(ini::load_flat::<CopilotSkill>);
    }
}
