//! Configuration loading and validation for all TOML config files.
pub mod category_matcher {
    //! Re-export of [`super::helpers::category_matcher`].
    pub use super::helpers::category_matcher::{Category, matches};
}
pub mod chmod;
pub mod copilot_plugins;
pub mod git_config;
pub(crate) mod helpers;
pub mod manifest;
pub mod packages;
pub mod profiles;
pub mod registry;
pub mod symlinks;
pub mod systemd_units;
pub mod vscode_extensions;

/// Define a [`ConfigSection`](helpers::toml_loader::ConfigSection) implementation
/// and `load()` function with minimal boilerplate.
///
/// Generates an internal section struct, the `ConfigSection` trait impl,
/// and a public `load()` function that filters by active categories.
///
/// # Syntax
///
/// ```ignore
/// // Identity mapping (Entry == Item):
/// config_section!(field: "settings", ty: GitSetting);
///
/// // Explicit mapping (Entry → Item):
/// config_section! {
///     field: "symlinks",
///     entry: SymlinkEntry,
///     item: Symlink,
///     map: |entry| match entry {
///         SymlinkEntry::Simple(s) => Symlink { source: s, target: None },
///         SymlinkEntry::WithTarget { source, target } => Symlink { source, target: Some(target) },
///     },
/// }
/// ```
macro_rules! config_section {
    // Identity mapping (Entry == Item).
    (field: $field:literal, ty: $ty:ty $(,)?) => {
        #[derive(Debug, ::serde::Deserialize)]
        struct Section {
            #[serde(rename = $field)]
            entries: Vec<$ty>,
        }

        impl $crate::config::helpers::toml_loader::ConfigSection for Section {
            type Entry = $ty;
            type Item = $ty;

            fn extract(self) -> Vec<$ty> {
                self.entries
            }

            fn map(entry: $ty) -> $ty {
                entry
            }
        }

        /// Load items from the TOML config file, filtered by active categories.
        ///
        /// # Errors
        ///
        /// Returns an error if the file exists but cannot be parsed.
        pub fn load(
            path: &::std::path::Path,
            active_categories: &[$crate::config::helpers::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$ty>> {
            $crate::config::helpers::toml_loader::load_section::<Section>(path, active_categories)
        }
    };

    // With explicit entry-to-item mapping.
    (
        field: $field:literal,
        entry: $entry:ty,
        item: $item:ty,
        map: |$param:ident| $map_expr:expr $(,)?
    ) => {
        #[derive(Debug, ::serde::Deserialize)]
        struct Section {
            #[serde(rename = $field)]
            entries: Vec<$entry>,
        }

        impl $crate::config::helpers::toml_loader::ConfigSection for Section {
            type Entry = $entry;
            type Item = $item;

            fn extract(self) -> Vec<$entry> {
                self.entries
            }

            fn map($param: $entry) -> $item {
                $map_expr
            }
        }

        /// Load items from the TOML config file, filtered by active categories.
        ///
        /// # Errors
        ///
        /// Returns an error if the file exists but cannot be parsed.
        pub fn load(
            path: &::std::path::Path,
            active_categories: &[$crate::config::helpers::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$item>> {
            $crate::config::helpers::toml_loader::load_section::<Section>(path, active_categories)
        }
    };
}

pub(crate) use config_section;

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
        loader: impl Fn(
            &std::path::Path,
            &[crate::config::category_matcher::Category],
        ) -> anyhow::Result<Vec<T>>,
    ) {
        use crate::config::category_matcher::Category;
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("nonexistent.toml");
        let result = loader(&path, &[Category::Base]).expect("loader should not fail");
        assert!(result.is_empty(), "missing file should produce empty list");
    }
}

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::platform::Platform;

/// A validation warning detected during configuration loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationWarning {
    /// The configuration source (e.g., "symlinks.toml", "packages.toml").
    pub source: String,
    /// The specific item or section that triggered the warning.
    pub item: String,
    /// Human-readable warning message.
    pub message: String,
}

impl ValidationWarning {
    /// Create a new validation warning.
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        item: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            item: item.into(),
            message: message.into(),
        }
    }
}

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
    /// GitHub Copilot plugins to install from marketplaces.
    pub copilot_plugins: Vec<copilot_plugins::CopilotPlugin>,
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
    pub fn load(root: &Path, profile: &profiles::Profile, platform: Platform) -> Result<Self> {
        let conf = root.join("conf");

        let active_categories = &profile.active_categories;
        let excluded_categories = &profile.excluded_categories;

        // Macro to build the path and attach the full path to any parse error,
        // removing the duplication of writing each filename twice.
        macro_rules! load_toml {
            // Two-argument form: loaders that do not filter by category (e.g. registry).
            ($file:literal, $loader:expr) => {{
                let path = conf.join($file);
                $loader(&path).with_context(|| format!("Invalid syntax in {}", path.display()))?
            }};
            // Three-argument form: loaders that accept a category slice for filtering.
            ($file:literal, $loader:expr, $cats:expr) => {{
                let path = conf.join($file);
                $loader(&path, $cats)
                    .with_context(|| format!("Invalid syntax in {}", path.display()))?
            }};
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
        let copilot_plugins = load_toml!(
            "copilot-plugins.toml",
            copilot_plugins::load,
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
            copilot_plugins,
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
    pub fn validate(&self, platform: Platform) -> Vec<ValidationWarning> {
        let root = &self.root;
        let mut warnings = Vec::new();
        warnings.extend(symlinks::validate(&self.symlinks, root));
        warnings.extend(packages::validate(&self.packages, platform));
        warnings.extend(registry::validate(&self.registry, platform));
        warnings.extend(chmod::validate(&self.chmod, platform));
        warnings.extend(systemd_units::validate(&self.units, platform));
        warnings.extend(vscode_extensions::validate(&self.vscode_extensions));
        warnings.extend(copilot_plugins::validate(&self.copilot_plugins));
        warnings.extend(git_config::validate(&self.git_settings));
        warnings
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
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
            "copilot-plugins.toml",
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
            active_categories: vec![Category::Base],
            excluded_categories: vec![Category::Desktop],
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
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
        assert!(config.packages.is_empty());
        assert!(config.symlinks.is_empty());
        assert!(config.registry.is_empty());
        assert!(config.units.is_empty());
        assert!(config.chmod.is_empty());
        assert!(config.vscode_extensions.is_empty());
        assert!(config.copilot_plugins.is_empty());
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
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
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
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
        assert_eq!(config.packages.len(), 2);
    }

    #[test]
    fn load_stores_root_path() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
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
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
        assert!(config.registry.is_empty(), "registry skipped on linux");
    }

    #[test]
    fn load_populates_systemd_units_on_linux() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("systemd-units.toml", "[base]\nunits = [\"ssh.service\"]\n")],
        );
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
        assert_eq!(config.units.len(), 1);
    }

    #[test]
    fn load_skips_systemd_units_on_windows() {
        let (dir, profile, platform) = setup_load(windows(), &[]);
        let config = Config::load(dir.path(), &profile, platform).expect("load should succeed");
        assert!(config.units.is_empty(), "systemd units skipped on windows");
    }

    #[test]
    fn load_returns_error_on_invalid_packages_toml() {
        let (dir, profile, platform) =
            setup_load(linux(), &[("packages.toml", "[base\npackages = [")]);
        let result = Config::load(dir.path(), &profile, platform);
        assert!(result.is_err(), "invalid packages.toml should return error");
        let msg = result.unwrap_err().to_string();
        let expected_path = dir.path().join("conf").join("packages.toml");
        assert!(
            msg.contains(expected_path.to_str().unwrap_or("packages.toml")),
            "error should mention the full path: {msg}"
        );
    }

    #[test]
    fn load_returns_error_on_invalid_git_config_toml() {
        let (dir, profile, platform) =
            setup_load(linux(), &[("git-config.toml", "not valid [[ toml")]);
        let result = Config::load(dir.path(), &profile, platform);
        assert!(
            result.is_err(),
            "invalid git-config.toml should return error"
        );
        let msg = result.unwrap_err().to_string();
        let expected_path = dir.path().join("conf").join("git-config.toml");
        assert!(
            msg.contains(expected_path.to_str().unwrap_or("git-config.toml")),
            "error should mention the full path: {msg}"
        );
    }

    #[test]
    fn load_returns_error_on_invalid_manifest_toml() {
        let (dir, profile, platform) = setup_load(linux(), &[("manifest.toml", "{{invalid}}")]);
        let result = Config::load(dir.path(), &profile, platform);
        assert!(result.is_err(), "invalid manifest.toml should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch_in_symlinks() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("symlinks.toml", "[base]\nsymlinks = \"not-an-array\"\n")],
        );
        let result = Config::load(dir.path(), &profile, platform);
        assert!(
            result.is_err(),
            "type mismatch in symlinks.toml should return error"
        );
    }
}
