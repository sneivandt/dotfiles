use anyhow::Result;
use std::path::Path;

use super::ini;

/// A font to validate/install.
#[derive(Debug, Clone)]
pub struct Font {
    pub name: String,
}

/// Load fonts from fonts.ini, filtered by active categories.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Font>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|name| Font { name })
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
    fn load_desktop_fonts() {
        let (_dir, path) = write_temp_ini("[desktop]\nNoto Color Emoji\nSauceCodePro Nerd Font\n");
        let fonts = load(&path, &["base".to_string(), "desktop".to_string()]).unwrap();
        assert_eq!(fonts.len(), 2);
        assert_eq!(fonts[0].name, "Noto Color Emoji");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let fonts = load(&path, &["base".to_string()]).unwrap();
        assert!(fonts.is_empty());
    }
}
