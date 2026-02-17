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
/// The "aur" tag is a package-type marker (not a profile category) and is
/// excluded from AND-logic filtering. It is only used to set `is_aur`.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Package>> {
    let sections = ini::parse_sections(path)?;

    let mut packages = Vec::new();
    for section in &sections {
        let is_aur = section.categories.contains(&"aur".to_string());
        // Filter on profile categories only (ignore "aur" marker)
        let dominated = section
            .categories
            .iter()
            .filter(|c| c.as_str() != "aur")
            .all(|cat| active_categories.contains(cat));
        if !dominated {
            continue;
        }
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
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn load_filters_by_category() {
        let (_dir, path) =
            write_temp_ini("[arch]\ngit\nvim\n\n[arch,aur]\nparu-bin\n\n[windows]\nwinget-pkg\n");
        let packages = load(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(packages.len(), 3);
        assert!(!packages[0].is_aur);
        assert_eq!(packages[0].name, "git");
        assert!(packages[2].is_aur);
        assert_eq!(packages[2].name, "paru-bin");
    }

    #[test]
    fn aur_packages_detected() {
        let (_dir, path) = write_temp_ini("[arch,aur]\nparu-bin\nyay\n");
        // "aur" is a package-type marker, not a profile category
        let packages = load(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages[0].is_aur);
        assert!(packages[1].is_aur);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let packages = load(&path, &["base".to_string()]).unwrap();
        assert!(
            packages.is_empty(),
            "missing file should produce empty list"
        );
    }
}
