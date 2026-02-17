use anyhow::Result;
use std::path::Path;

use super::ini;

/// A GitHub Copilot skill URL.
#[derive(Debug, Clone)]
pub struct CopilotSkill {
    pub url: String,
}

/// Load Copilot skills from copilot-skills.ini, filtered by active categories.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<CopilotSkill>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|url| CopilotSkill { url })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_ini(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ini");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

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
        assert!(skills.is_empty());
    }
}
