use anyhow::Result;
use std::path::Path;

use super::ini;

/// A GitHub Copilot skill URL.
#[derive(Debug, Clone)]
pub struct CopilotSkill {
    /// GitHub blob or tree URL pointing to the skill directory.
    pub url: String,
}

/// Load Copilot skills from copilot-skills.ini, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<CopilotSkill>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|url| CopilotSkill { url })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn load_base_skills() {
        let (_dir, path) = write_temp_ini(
            "[base]\nhttps://github.com/example/skill1\nhttps://github.com/example/skill2\n",
        );
        let skills = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills[0].url.starts_with("https://"));
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let skills = load(&path, &["base".to_string()]).unwrap();
        assert!(skills.is_empty(), "missing file should produce empty list");
    }
}
