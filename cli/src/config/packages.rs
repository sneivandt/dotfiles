//! Package configuration loading.
use serde::Deserialize;

use super::ValidationWarning;
use super::config_section;

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

config_section! {
    field: "packages",
    entry: PackageEntry,
    item: Package,
    map: |entry| match entry {
        PackageEntry::Simple(name) => Package {
            name,
            is_aur: false,
        },
        PackageEntry::WithMetadata { name, aur } => Package {
            name,
            is_aur: aur.unwrap_or(false),
        },
    },
}

/// Validate package entries and return any warnings.
#[must_use]
pub fn validate(
    packages: &[Package],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    Validator::new("packages.toml")
        .check_each(
            packages,
            |pkg| &pkg.name,
            |pkg| {
                vec![
                    check(
                        pkg.is_aur && !platform.is_arch_linux(),
                        "AUR package specified but platform is not Arch Linux",
                    ),
                    check(pkg.name.trim().is_empty(), "package name is empty"),
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

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\npackages = [\"git\"");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\npackages = 42\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
    }

    #[test]
    fn validate_warns_empty_package_name() {
        use crate::platform::{Os, Platform};

        let packages = vec![Package {
            name: "  ".to_string(),
            is_aur: false,
        }];
        let warnings = validate(&packages, Platform::new(Os::Linux, false));
        assert!(
            warnings.iter().any(|w| w.message.contains("empty")),
            "should warn about empty package name: {warnings:?}"
        );
    }
}
