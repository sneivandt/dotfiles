pub mod category_matcher;
pub mod chmod;
pub mod copilot_skills;
pub mod ini;
pub mod manifest;
pub mod packages;
pub mod profiles;
pub mod registry;
pub mod symlinks;
pub mod systemd_units;
pub mod validation;
pub mod vscode_extensions;

/// Define a simple config struct with a single `String` field and a
/// `From<String>` conversion, used by config types loaded via
/// [`ini::load_flat`].
///
/// # Examples
///
/// ```ignore
/// define_flat_config! {
///     /// A symlink to create.
///     Symlink {
///         /// Relative path under symlinks/ directory.
///         source
///     }
/// }
/// // expands to:
/// //   #[derive(Debug, Clone)]
/// //   pub struct Symlink { pub source: String }
/// //   impl From<String> for Symlink { ... }
/// ```
macro_rules! define_flat_config {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(#[$field_meta:meta])*
            $field:ident
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name {
            $(#[$field_meta])*
            pub $field: String,
        }

        impl From<String> for $name {
            fn from($field: String) -> Self {
                Self { $field }
            }
        }
    };
}

pub(crate) use define_flat_config;

#[cfg(test)]
pub mod test_helpers {
    use std::path::PathBuf;

    /// Write content to a temp INI file and return the temp dir + path.
    /// The `TempDir` must be kept alive for the file to persist during the test.
    ///
    /// # Panics
    ///
    /// Panics if the temp directory or file cannot be created.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn write_temp_ini(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.ini");
        std::fs::write(&path, content).expect("failed to write temp ini");
        (dir, path)
    }

    /// Assert that a config loader returns an empty list for a missing file.
    ///
    /// Eliminates the repeated pattern of creating a temp dir, pointing at a
    /// nonexistent file, calling the loader, and asserting the result is empty.
    ///
    /// # Panics
    ///
    /// Panics if the temp directory cannot be created or the loader fails.
    #[allow(clippy::expect_used)]
    pub fn assert_load_missing_returns_empty<T>(
        loader: impl Fn(&std::path::Path, &[String]) -> anyhow::Result<Vec<T>>,
    ) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("nonexistent.ini");
        let result = loader(&path, &["base".to_string()]).expect("loader should not fail");
        assert!(result.is_empty(), "missing file should produce empty list");
    }
}

use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::platform::Platform;

/// All loaded configuration for a resolved profile.
#[derive(Debug)]
pub struct Config {
    /// Root directory of the dotfiles repository.
    pub root: PathBuf,
    /// The resolved profile (retained for debug output via `Debug` impl).
    #[allow(dead_code)]
    pub profile: profiles::Profile,
    /// Packages to install via system package managers.
    pub packages: Vec<packages::Package>,
    /// Symlinks to create in the user's home directory.
    pub symlinks: Vec<symlinks::Symlink>,
    /// Windows registry entries to configure.
    pub registry: Vec<registry::RegistryEntry>,
    /// Systemd user units to enable.
    pub units: Vec<systemd_units::SystemdUnit>,
    /// File permissions to apply (chmod).
    pub chmod: Vec<chmod::ChmodEntry>,
    /// VS Code extensions to install.
    pub vscode_extensions: Vec<vscode_extensions::VsCodeExtension>,
    /// GitHub Copilot skills to clone.
    pub copilot_skills: Vec<copilot_skills::CopilotSkill>,
    /// Sparse checkout manifest for file exclusions.
    pub manifest: manifest::Manifest,
}

impl Config {
    /// Load all configuration for the given profile from the conf/ directory.
    ///
    /// # Errors
    ///
    /// Returns an error if any configuration file cannot be parsed.
    pub fn load(
        root: &Path,
        profile: &profiles::Profile,
        platform: &Platform,
    ) -> Result<Self, ConfigError> {
        let conf = root.join("conf");

        let active_categories = &profile.active_categories;
        let excluded_categories = &profile.excluded_categories;

        let packages =
            packages::load(&conf.join("packages.ini"), active_categories).map_err(|e| {
                ConfigError::InvalidSyntax {
                    file: "packages.ini".to_string(),
                    message: e.to_string(),
                }
            })?;

        let symlinks =
            ini::load_flat(&conf.join("symlinks.ini"), active_categories).map_err(|e| {
                ConfigError::InvalidSyntax {
                    file: "symlinks.ini".to_string(),
                    message: e.to_string(),
                }
            })?;

        let registry = if platform.has_registry() {
            registry::load(&conf.join("registry.ini")).map_err(|e| ConfigError::InvalidSyntax {
                file: "registry.ini".to_string(),
                message: e.to_string(),
            })?
        } else {
            Vec::new()
        };

        let units = if platform.supports_systemd() {
            ini::load_flat(&conf.join("systemd-units.ini"), active_categories).map_err(|e| {
                ConfigError::InvalidSyntax {
                    file: "systemd-units.ini".to_string(),
                    message: e.to_string(),
                }
            })?
        } else {
            Vec::new()
        };

        let chmod_entries =
            chmod::load(&conf.join("chmod.ini"), active_categories).map_err(|e| {
                ConfigError::InvalidSyntax {
                    file: "chmod.ini".to_string(),
                    message: e.to_string(),
                }
            })?;

        let vscode_extensions =
            ini::load_flat(&conf.join("vscode-extensions.ini"), active_categories).map_err(
                |e| ConfigError::InvalidSyntax {
                    file: "vscode-extensions.ini".to_string(),
                    message: e.to_string(),
                },
            )?;

        let copilot_skills = ini::load_flat(&conf.join("copilot-skills.ini"), active_categories)
            .map_err(|e| ConfigError::InvalidSyntax {
                file: "copilot-skills.ini".to_string(),
                message: e.to_string(),
            })?;

        let manifest =
            manifest::load(&conf.join("manifest.ini"), excluded_categories).map_err(|e| {
                ConfigError::InvalidSyntax {
                    file: "manifest.ini".to_string(),
                    message: e.to_string(),
                }
            })?;

        Ok(Self {
            root: root.to_path_buf(),
            profile: profile.clone(),
            packages,
            symlinks,
            registry,
            units,
            chmod: chmod_entries,
            vscode_extensions,
            copilot_skills,
            manifest,
        })
    }

    /// Validate the configuration and return any warnings.
    ///
    /// This method checks for common configuration issues such as:
    /// - Missing source files for symlinks
    /// - Invalid values (e.g., invalid octal modes for chmod)
    /// - Platform incompatibilities
    #[must_use]
    pub fn validate(&self, platform: &Platform) -> Vec<validation::ValidationWarning> {
        validation::validate_all(self, platform)
    }
}
