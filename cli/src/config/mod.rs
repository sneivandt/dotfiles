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
}

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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
    pub fn load(root: &Path, profile: &profiles::Profile, platform: &Platform) -> Result<Self> {
        let conf = root.join("conf");

        let active_categories = &profile.active_categories;
        let excluded_categories = &profile.excluded_categories;

        let packages = packages::load(&conf.join("packages.ini"), active_categories)
            .context("loading packages.ini")?;

        let symlinks = symlinks::load(&conf.join("symlinks.ini"), active_categories)
            .context("loading symlinks.ini")?;

        let registry = if platform.has_registry() {
            registry::load(&conf.join("registry.ini")).context("loading registry.ini")?
        } else {
            Vec::new()
        };

        let units = if platform.supports_systemd() {
            systemd_units::load(&conf.join("systemd-units.ini"), active_categories)
                .context("loading systemd-units.ini")?
        } else {
            Vec::new()
        };

        let chmod_entries =
            chmod::load(&conf.join("chmod.ini"), active_categories).context("loading chmod.ini")?;

        let vscode_extensions =
            vscode_extensions::load(&conf.join("vscode-extensions.ini"), active_categories)
                .context("loading vscode-extensions.ini")?;

        let copilot_skills =
            copilot_skills::load(&conf.join("copilot-skills.ini"), active_categories)
                .context("loading copilot-skills.ini")?;

        let manifest = manifest::load(&conf.join("manifest.ini"), excluded_categories)
            .context("loading manifest.ini")?;

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
