use anyhow::Result;
use std::path::Path;

use super::ini;

/// A package to install.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub is_aur: bool,
}

/// Load packages from packages.ini, filtered by active categories.
///
/// Sections with "aur" in their categories are AUR packages (installed via paru).
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Package>> {
    let sections = ini::parse_sections(path)?;
    let filtered = ini::filter_sections_and(&sections, active_categories);

    let mut packages = Vec::new();
    for section in &filtered {
        let is_aur = section.categories.contains(&"aur".to_string());
        for item in &section.items {
            packages.push(Package {
                name: item.clone(),
                is_aur,
            });
        }
    }

    Ok(packages)
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
    fn load_filters_by_category() {
        let (_dir, path) =
            write_temp_ini("[arch]\ngit\nvim\n\n[arch,aur]\nparu-bin\n\n[windows]\nwinget-pkg\n");
        let packages = load(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(packages.len(), 2);
        assert!(!packages[0].is_aur);
        assert_eq!(packages[0].name, "git");
    }

    #[test]
    fn aur_packages_detected() {
        let (_dir, path) = write_temp_ini("[arch,aur]\nparu-bin\nyay\n");
        let packages = load(
            &path,
            &["base".to_string(), "arch".to_string(), "aur".to_string()],
        )
        .unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages[0].is_aur);
        assert!(packages[1].is_aur);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let packages = load(&path, &["base".to_string()]).unwrap();
        assert!(packages.is_empty());
    }
}
