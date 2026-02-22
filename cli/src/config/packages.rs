use anyhow::Result;
use std::path::Path;

use super::category_matcher::MatchMode;
use super::ini;

/// A package to install.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name or identifier (e.g., "git", "Git.Git" for winget).
    pub name: String,
    /// Whether this is an AUR (Arch User Repository) package.
    pub is_aur: bool,
}

/// Load packages from packages.ini, filtered by active categories.
///
/// Packages prefixed with `aur:` are tagged with `is_aur = true` and the
/// prefix is stripped from the package name.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Package>> {
    let sections = ini::parse_sections(path)?;
    let filtered = ini::filter_sections(&sections, active_categories, MatchMode::All);

    Ok(filtered
        .into_iter()
        .flat_map(|s| {
            s.items.into_iter().map(|item| {
                let (name, is_aur) = item
                    .strip_prefix("aur:")
                    .map_or((item.as_str(), false), |n| (n, true));
                Package {
                    name: name.to_string(),
                    is_aur,
                }
            })
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn load_filters_by_category() {
        let (_dir, path) =
            write_temp_ini("[arch]\ngit\nvim\naur:paru-bin\n\n[windows]\nwinget-pkg\n");
        let packages = load(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(packages.len(), 3);
        assert!(!packages[0].is_aur);
        assert_eq!(packages[0].name, "git");
        assert!(packages[2].is_aur);
        assert_eq!(packages[2].name, "paru-bin");
    }

    #[test]
    fn aur_packages_detected() {
        let (_dir, path) = write_temp_ini("[arch]\naur:paru-bin\naur:yay\n");
        let packages = load(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages[0].is_aur);
        assert!(packages[1].is_aur);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
