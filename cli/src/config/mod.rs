//! Configuration loading and validation for all TOML config files.
pub mod category_matcher;
pub mod chmod;
pub mod copilot_skills;
pub mod git_config;
pub mod manifest;
pub mod packages;
pub mod profiles;
pub mod registry;
pub mod symlinks;
pub mod systemd_units;
pub mod toml_loader;
pub mod validation;
pub mod vscode_extensions;

#[cfg(test)]
/// Test helpers for config module tests.
pub mod test_helpers {
    use std::path::PathBuf;

    /// Write content to a temp TOML file and return the temp dir + path.
    /// The `TempDir` must be kept alive for the file to persist during the test.
    ///
    /// # Panics
    ///
    /// Panics if the temp directory or file cannot be created.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn write_temp_toml(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.toml");
        std::fs::write(&path, content).expect("failed to write temp toml");
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
        let path = dir.path().join("nonexistent.toml");
        let result = loader(&path, &["base".to_string()]).expect("loader should not fail");
        assert!(result.is_empty(), "missing file should produce empty list");
    }
}

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::platform::Platform;

/// All loaded configuration for a resolved profile.
#[derive(Debug)]
pub struct Config {
    /// Root directory of the dotfiles repository.
    pub root: PathBuf,
    /// The resolved profile, used to reload configuration after repository updates.
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
    /// Git configuration settings to apply globally.
    pub git_settings: Vec<git_config::GitSetting>,
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

        // Macro to build the path and attach the filename to any parse error,
        // removing the duplication of writing each filename twice.
        macro_rules! load_toml {
            // Two-argument form: loaders that do not filter by category (e.g. registry).
            ($file:literal, $loader:expr) => {
                $loader(&conf.join($file))
                    .with_context(|| format!("Invalid syntax in {}", $file))?
            };
            // Three-argument form: loaders that accept a category slice for filtering.
            ($file:literal, $loader:expr, $cats:expr) => {
                $loader(&conf.join($file), $cats)
                    .with_context(|| format!("Invalid syntax in {}", $file))?
            };
        }

        let packages = load_toml!("packages.toml", packages::load, active_categories);
        let symlinks = load_toml!("symlinks.toml", symlinks::load, active_categories);

        let registry = if platform.has_registry() {
            load_toml!("registry.toml", registry::load)
        } else {
            Vec::new()
        };

        let units = if platform.supports_systemd() {
            load_toml!("systemd-units.toml", systemd_units::load, active_categories)
        } else {
            Vec::new()
        };

        let chmod_entries = load_toml!("chmod.toml", chmod::load, active_categories);
        let vscode_extensions = load_toml!(
            "vscode-extensions.toml",
            vscode_extensions::load,
            active_categories
        );
        let copilot_skills = load_toml!(
            "copilot-skills.toml",
            copilot_skills::load,
            active_categories
        );
        let git_settings = load_toml!("git-config.toml", git_config::load, active_categories);
        let manifest = load_toml!("manifest.toml", manifest::load, excluded_categories);

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
            git_settings,
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::{Os, Platform};

    /// Create a temporary directory tree with the minimal conf/ files required
    /// by `Config::load` and return the `TempDir` (keep alive) + profile.
    fn setup_load(
        platform: Platform,
        overrides: &[(&str, &str)],
    ) -> (tempfile::TempDir, profiles::Profile, Platform) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let conf = dir.path().join("conf");
        std::fs::create_dir_all(&conf).expect("create conf dir");

        for file in &[
            "packages.toml",
            "symlinks.toml",
            "registry.toml",
            "systemd-units.toml",
            "chmod.toml",
            "vscode-extensions.toml",
            "copilot-skills.toml",
            "git-config.toml",
            "manifest.toml",
        ] {
            std::fs::write(conf.join(file), "").expect("write empty toml");
        }

        for (name, content) in overrides {
            std::fs::write(conf.join(name), content).expect("write override toml");
        }

        let profile = profiles::Profile {
            name: "base".to_string(),
            active_categories: vec!["base".to_string()],
            excluded_categories: vec!["desktop".to_string()],
        };
        (dir, profile, platform)
    }

    fn linux() -> Platform {
        Platform::new(Os::Linux, false)
    }

    fn windows() -> Platform {
        Platform::new(Os::Windows, false)
    }

    #[test]
    fn load_with_empty_config_files() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert!(config.packages.is_empty());
        assert!(config.symlinks.is_empty());
        assert!(config.registry.is_empty());
        assert!(config.units.is_empty());
        assert!(config.chmod.is_empty());
        assert!(config.vscode_extensions.is_empty());
        assert!(config.copilot_skills.is_empty());
    }

    #[test]
    fn load_populates_symlinks() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[(
                "symlinks.toml",
                "[base]\nsymlinks = [\".bashrc\", \".vimrc\"]\n",
            )],
        );
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert_eq!(config.symlinks.len(), 2);
        assert_eq!(config.symlinks[0].source, ".bashrc");
        assert_eq!(config.symlinks[1].source, ".vimrc");
    }

    #[test]
    fn load_populates_packages() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("packages.toml", "[base]\npackages = [\"git\", \"curl\"]\n")],
        );
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert_eq!(config.packages.len(), 2);
    }

    #[test]
    fn load_stores_root_path() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert_eq!(config.root, dir.path());
    }

    #[test]
    fn load_skips_registry_on_linux() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[(
                "registry.toml",
                "[test]\npath = \"HKCU:\\\\Test\"\n[test.values]\nKey = \"Value\"\n",
            )],
        );
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert!(config.registry.is_empty(), "registry skipped on linux");
    }

    #[test]
    fn load_populates_systemd_units_on_linux() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("systemd-units.toml", "[base]\nunits = [\"ssh.service\"]\n")],
        );
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert_eq!(config.units.len(), 1);
    }

    #[test]
    fn load_skips_systemd_units_on_windows() {
        let (dir, profile, platform) = setup_load(windows(), &[]);
        let config = Config::load(dir.path(), &profile, &platform).expect("load should succeed");
        assert!(config.units.is_empty(), "systemd units skipped on windows");
    }
}
