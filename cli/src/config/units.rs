use anyhow::Result;
use std::path::Path;

use super::ini;

/// A systemd user unit to enable.
#[derive(Debug, Clone)]
pub struct Unit {
    pub name: String,
}

/// Load systemd units from units.ini, filtered by active categories.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Unit>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|name| Unit { name })
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
    fn load_base_units() {
        let (_dir, path) =
            write_temp_ini("[base]\nclean-home-tmp.timer\n\n[arch,desktop]\ndunst.service\n");
        let units = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "clean-home-tmp.timer");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let units = load(&path, &["base".to_string()]).unwrap();
        assert!(units.is_empty());
    }
}
