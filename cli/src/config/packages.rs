//! Package configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

/// A package to install.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name or identifier (e.g., "git", "Git.Git" for winget).
    pub name: String,
    /// Whether this is an AUR (Arch User Repository) package.
    pub is_aur: bool,
}

/// TOML package entry - can be either a string or a table with metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PackageEntry {
    Simple(String),
    WithMetadata { name: String, aur: Option<bool> },
}

/// TOML section containing packages.
#[derive(Debug, Deserialize)]
struct PackageSection {
    packages: Vec<PackageEntry>,
}

/// Load packages from packages.toml, filtered by active categories.
///
/// Packages can be simple strings or objects with metadata. The `aur`
/// field marks a package as an AUR package.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<Package>> {
    let items = toml_loader::load_section_items(path, |s: PackageSection| s.packages)?;

    let entries: Vec<PackageEntry> =
        toml_loader::filter_by_categories(items, active_categories, MatchMode::All);

    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            PackageEntry::Simple(name) => Package {
                name,
                is_aur: false,
            },
            PackageEntry::WithMetadata { name, aur } => Package {
                name,
                is_aur: aur.unwrap_or(false),
            },
        })
        .collect())
}

/// Validate package entries and return any warnings.
#[must_use]
pub fn validate(
    packages: &[Package],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    for package in packages {
        if package.is_aur && !platform.is_arch_linux() {
            warnings.push(ValidationWarning::new(
                "packages.toml",
                &package.name,
                "AUR package specified but platform is not Arch Linux",
            ));
        }

        if package.name.trim().is_empty() {
            warnings.push(ValidationWarning::new(
                "packages.toml",
                &package.name,
                "package name is empty",
            ));
        }
    }

    warnings
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_filters_by_category() {
        let (_dir, path) = write_temp_toml(
            r#"[arch]
packages = ["git", "vim", { name = "paru-bin", aur = true }]

[windows]
packages = ["winget-pkg"]
"#,
        );
        let packages = load(&path, &[Category::Base, Category::Arch]).unwrap();
        assert_eq!(packages.len(), 3);
        assert!(!packages[0].is_aur);
        assert_eq!(packages[0].name, "git");
        assert!(packages[2].is_aur);
        assert_eq!(packages[2].name, "paru-bin");
    }

    #[test]
    fn aur_packages_detected() {
        let (_dir, path) = write_temp_toml(
            r#"[arch]
packages = [{ name = "paru-bin", aur = true }, { name = "yay", aur = true }]
"#,
        );
        let packages = load(&path, &[Category::Base, Category::Arch]).unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages[0].is_aur);
        assert!(packages[1].is_aur);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn validate_warns_aur_on_non_arch() {
        use crate::platform::{Os, Platform};

        let packages = vec![Package {
            name: "yay".to_string(),
            is_aur: true,
        }];
        let warnings = validate(&packages, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("not Arch Linux"));
    }
}
