//! Configuration loading and validation for all TOML config files.
pub mod category_matcher {
    //! Re-export of [`super::helpers::category_matcher`].
    pub use super::helpers::category_matcher::{Category, matches};
}
pub(crate) mod apm;
pub mod chmod;
pub mod copilot;
pub mod git_config;
pub(crate) mod helpers;
pub mod manifest;
pub mod overlay;
pub mod packages;
pub mod profiles;
pub mod registry;
pub mod scripts;
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
        #[serde(deny_unknown_fields)]
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
        #[serde(deny_unknown_fields)]
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
    #[allow(
        clippy::expect_used,
        reason = "panicking allowed at this trust boundary"
    )]
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
    #[allow(
        clippy::expect_used,
        reason = "panicking allowed at this trust boundary"
    )]
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

const PACKAGES_TOML: &str = "packages.toml";
const SYMLINKS_TOML: &str = "symlinks.toml";
const REGISTRY_TOML: &str = "registry.toml";
const SYSTEMD_UNITS_TOML: &str = "systemd-units.toml";
const CHMOD_TOML: &str = "chmod.toml";
const VSCODE_EXTENSIONS_TOML: &str = "vscode-extensions.toml";
const GIT_CONFIG_TOML: &str = "git-config.toml";
const COPILOT_TOML: &str = "copilot.toml";
const MANIFEST_TOML: &str = "manifest.toml";
const SCRIPTS_TOML: &str = "scripts.toml";

#[derive(Debug, Clone, Copy)]
enum ConfigSource {
    Main,
    Overlay,
}

#[derive(Debug)]
struct ConfigLoader {
    conf_dir: PathBuf,
    source: ConfigSource,
}

impl ConfigLoader {
    fn main(root: &Path) -> Self {
        Self {
            conf_dir: root.join("conf"),
            source: ConfigSource::Main,
        }
    }

    fn overlay(root: &Path) -> Self {
        Self {
            conf_dir: root.join("conf"),
            source: ConfigSource::Overlay,
        }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.conf_dir.join(file)
    }

    fn error_context(&self, path: &Path) -> String {
        match self.source {
            ConfigSource::Main => format!("Invalid syntax in {}", path.display()),
            ConfigSource::Overlay => format!("Invalid syntax in overlay {}", path.display()),
        }
    }

    fn load<T>(&self, file: &str, loader: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        let path = self.path(file);
        loader(&path).with_context(|| self.error_context(&path))
    }

    fn load_filtered<T>(
        &self,
        file: &str,
        loader: impl FnOnce(&Path, &[category_matcher::Category]) -> Result<T>,
        categories: &[category_matcher::Category],
    ) -> Result<T> {
        let path = self.path(file);
        loader(&path, categories).with_context(|| self.error_context(&path))
    }

    fn load_overlay<T: Default>(
        &self,
        file: &str,
        loader: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<T> {
        let path = self.path(file);
        if path.exists() {
            loader(&path).with_context(|| self.error_context(&path))
        } else {
            Ok(T::default())
        }
    }

    fn load_overlay_filtered<T: Default>(
        &self,
        file: &str,
        loader: impl FnOnce(&Path, &[category_matcher::Category]) -> Result<T>,
        categories: &[category_matcher::Category],
    ) -> Result<T> {
        let path = self.path(file);
        if path.exists() {
            loader(&path, categories).with_context(|| self.error_context(&path))
        } else {
            Ok(T::default())
        }
    }
}

#[derive(Debug)]
struct ConfigValidator<'a> {
    config: &'a Config,
    platform: Platform,
    warnings: Vec<ValidationWarning>,
}

impl<'a> ConfigValidator<'a> {
    const fn new(config: &'a Config, platform: Platform) -> Self {
        Self {
            config,
            platform,
            warnings: Vec::new(),
        }
    }

    fn validate_all(self) -> Self {
        self.validate_with(|config, _platform| symlinks::validate(&config.symlinks, &config.root))
            .validate_with(|config, _platform| {
                apm::validate(&config.root, config.overlay.as_deref())
            })
            .validate_with(|config, platform| packages::validate(&config.packages, platform))
            .validate_with(|config, platform| registry::validate(&config.registry, platform))
            .validate_with(|config, platform| chmod::validate(&config.chmod, platform))
            .validate_with(|config, platform| systemd_units::validate(&config.units, platform))
            .validate_with(|config, _platform| {
                vscode_extensions::validate(&config.vscode_extensions)
            })
            .validate_with(|config, _platform| git_config::validate(&config.git_settings))
            .validate_with(|config, _platform| copilot::validate(&config.copilot_settings))
    }

    fn validate_with(
        mut self,
        validate: impl FnOnce(&Config, Platform) -> Vec<ValidationWarning>,
    ) -> Self {
        self.warnings.extend(validate(self.config, self.platform));
        self
    }

    fn finish(self) -> Vec<ValidationWarning> {
        self.warnings
    }
}

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
    /// Optional path to a private overlay repository.
    pub overlay: Option<PathBuf>,
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
    /// Git configuration settings to apply globally.
    pub git_settings: Vec<git_config::GitSetting>,
    /// GitHub Copilot CLI settings to converge in `~/.copilot/settings.json`.
    pub copilot_settings: Vec<copilot::CopilotSetting>,
    /// Sparse checkout manifest for file exclusions.
    pub manifest: manifest::Manifest,
    /// Custom scripts from the overlay repository.
    pub scripts: Vec<scripts::ScriptEntry>,
}

impl Config {
    /// Load all configuration for the given profile from the conf/ directory,
    /// optionally merging additional configuration from an overlay repository.
    ///
    /// # Errors
    ///
    /// Returns an error if any configuration file cannot be parsed.
    pub fn load(
        root: &Path,
        profile: &profiles::Profile,
        platform: Platform,
        overlay: Option<&Path>,
    ) -> Result<Self> {
        let active_categories = &profile.active_categories;
        let excluded_categories = &profile.excluded_categories;
        let loader = ConfigLoader::main(root);

        let packages = loader.load_filtered(PACKAGES_TOML, packages::load, active_categories)?;
        let mut symlinks =
            loader.load_filtered(SYMLINKS_TOML, symlinks::load, active_categories)?;
        symlinks::set_origin(&mut symlinks, root);

        let registry = if platform.has_registry() {
            loader.load(REGISTRY_TOML, registry::load)?
        } else {
            Vec::new()
        };

        let units = if platform.supports_systemd() {
            loader.load_filtered(SYSTEMD_UNITS_TOML, systemd_units::load, active_categories)?
        } else {
            Vec::new()
        };

        let chmod_entries = loader.load_filtered(CHMOD_TOML, chmod::load, active_categories)?;
        let vscode_extensions = loader.load_filtered(
            VSCODE_EXTENSIONS_TOML,
            vscode_extensions::load,
            active_categories,
        )?;
        let git_settings =
            loader.load_filtered(GIT_CONFIG_TOML, git_config::load, active_categories)?;
        let copilot_settings =
            loader.load_filtered(COPILOT_TOML, copilot::load, active_categories)?;
        let manifest = loader.load_filtered(MANIFEST_TOML, manifest::load, excluded_categories)?;

        let mut config = Self {
            root: root.to_path_buf(),
            overlay: overlay.map(Path::to_path_buf),
            profile: profile.clone(),
            packages,
            symlinks,
            registry,
            units,
            chmod: chmod_entries,
            vscode_extensions,
            git_settings,
            copilot_settings,
            manifest,
            scripts: Vec::new(),
        };

        // Merge overlay configuration if an overlay path is provided.
        if let Some(overlay_root) = overlay {
            config.merge_overlay(overlay_root, active_categories, platform)?;
        }

        config.symlinks = symlinks::expand_glob_patterns(&config.symlinks, root)
            .context("expanding symlink glob patterns")?;

        Ok(config)
    }

    /// Merge configuration from an overlay repository into this config.
    ///
    /// Overlay TOML files use the same format as the main `conf/` files and
    /// are appended to the existing lists.
    ///
    /// # Errors
    ///
    /// Returns an error if any overlay configuration file cannot be parsed.
    fn merge_overlay(
        &mut self,
        overlay_root: &Path,
        active_categories: &[category_matcher::Category],
        platform: Platform,
    ) -> Result<()> {
        let loader = ConfigLoader::overlay(overlay_root);

        self.packages.extend(loader.load_overlay_filtered(
            PACKAGES_TOML,
            packages::load,
            active_categories,
        )?);
        self.symlinks.extend({
            let mut overlay_symlinks =
                loader.load_overlay_filtered(SYMLINKS_TOML, symlinks::load, active_categories)?;
            symlinks::set_origin(&mut overlay_symlinks, overlay_root);
            overlay_symlinks
        });
        if platform.has_registry() {
            self.registry
                .extend(loader.load_overlay(REGISTRY_TOML, registry::load)?);
        }
        if platform.supports_systemd() {
            self.units.extend(loader.load_overlay_filtered(
                SYSTEMD_UNITS_TOML,
                systemd_units::load,
                active_categories,
            )?);
        }
        self.chmod.extend(loader.load_overlay_filtered(
            CHMOD_TOML,
            chmod::load,
            active_categories,
        )?);
        self.vscode_extensions.extend(loader.load_overlay_filtered(
            VSCODE_EXTENSIONS_TOML,
            vscode_extensions::load,
            active_categories,
        )?);
        self.git_settings.extend(loader.load_overlay_filtered(
            GIT_CONFIG_TOML,
            git_config::load,
            active_categories,
        )?);
        self.copilot_settings.extend(loader.load_overlay_filtered(
            COPILOT_TOML,
            copilot::load,
            active_categories,
        )?);
        self.scripts.extend(loader.load_overlay_filtered(
            SCRIPTS_TOML,
            scripts::load,
            active_categories,
        )?);

        Ok(())
    }

    /// Validate the configuration and return any warnings.
    ///
    /// This method checks for common configuration issues such as:
    /// - Missing source files for symlinks
    /// - Invalid values (e.g., invalid octal modes for chmod)
    /// - Platform incompatibilities
    #[must_use]
    pub fn validate(&self, platform: Platform) -> Vec<ValidationWarning> {
        ConfigValidator::new(self, platform).validate_all().finish()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
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

    fn write_overlay_config(overlay: &tempfile::TempDir, file: &str, content: &str) -> PathBuf {
        let conf = overlay.path().join("conf");
        std::fs::create_dir_all(&conf).expect("create overlay conf");
        let path = conf.join(file);
        std::fs::write(&path, content).expect("write overlay config");
        path
    }

    #[test]
    fn load_with_empty_config_files() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert!(config.packages.is_empty());
        assert!(config.symlinks.is_empty());
        assert!(config.registry.is_empty());
        assert!(config.units.is_empty());
        assert!(config.chmod.is_empty());
        assert!(config.vscode_extensions.is_empty());
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
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert_eq!(config.symlinks.len(), 2);
        assert_eq!(config.symlinks[0].source, ".bashrc");
        assert_eq!(config.symlinks[1].source, ".vimrc");
    }

    #[test]
    fn load_expands_overlay_symlink_globs() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let overlay = tempfile::tempdir().expect("create overlay dir");
        write_overlay_config(
            &overlay,
            "symlinks.toml",
            "[base]\nsymlinks = [{ source = \"skills/*\", target = \".copilot/skills/*\" }]\n",
        );
        std::fs::create_dir_all(
            overlay
                .path()
                .join("symlinks")
                .join("skills")
                .join("authz-oncall"),
        )
        .expect("create overlay skill");

        let config = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
            .expect("load should succeed");
        assert_eq!(config.symlinks.len(), 1);
        assert_eq!(config.symlinks[0].source, "skills/authz-oncall");
        assert_eq!(
            config.symlinks[0].target.as_deref(),
            Some(".copilot/skills/authz-oncall")
        );
        assert_eq!(config.symlinks[0].origin.as_deref(), Some(overlay.path()));
    }

    #[test]
    fn load_appends_overlay_packages_and_scripts() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("packages.toml", "[base]\npackages = [\"git\"]\n")],
        );
        let overlay = tempfile::tempdir().expect("create overlay dir");
        write_overlay_config(&overlay, "packages.toml", "[base]\npackages = [\"curl\"]\n");
        write_overlay_config(
            &overlay,
            "scripts.toml",
            r#"
[base]
scripts = [{ name = "Setup SSH", path = "scripts/ssh.sh" }]

[desktop]
scripts = [{ name = "Setup desktop", path = "scripts/desktop.sh" }]
"#,
        );

        let config = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
            .expect("load should succeed");

        assert_eq!(config.overlay.as_deref(), Some(overlay.path()));
        assert_eq!(
            config
                .packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>(),
            vec!["git", "curl"],
            "overlay packages should append to main packages"
        );
        assert_eq!(config.scripts.len(), 1);
        assert_eq!(config.scripts[0].name, "Setup SSH");
        assert_eq!(config.scripts[0].path, "scripts/ssh.sh");
    }

    #[test]
    fn load_reports_overlay_path_for_overlay_syntax_errors() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let overlay = tempfile::tempdir().expect("create overlay dir");
        let invalid_path = write_overlay_config(&overlay, "scripts.toml", "[base\nscripts = [");

        let result = Config::load(dir.path(), &profile, platform, Some(overlay.path()));

        assert!(result.is_err(), "invalid overlay config should fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("overlay"),
            "error should identify overlay config source: {msg}"
        );
        assert!(
            msg.contains(invalid_path.to_str().unwrap_or("scripts.toml")),
            "error should include overlay config path: {msg}"
        );
    }

    #[test]
    fn load_populates_packages() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("packages.toml", "[base]\npackages = [\"git\", \"curl\"]\n")],
        );
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert_eq!(config.packages.len(), 2);
    }

    #[test]
    fn load_stores_root_path() {
        let (dir, profile, platform) = setup_load(linux(), &[]);
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
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
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert!(config.registry.is_empty(), "registry skipped on linux");
    }

    #[test]
    fn load_populates_systemd_units_on_linux() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("systemd-units.toml", "[base]\nunits = [\"ssh.service\"]\n")],
        );
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert_eq!(config.units.len(), 1);
    }

    #[test]
    fn load_skips_systemd_units_on_windows() {
        let (dir, profile, platform) = setup_load(windows(), &[]);
        let config =
            Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
        assert!(config.units.is_empty(), "systemd units skipped on windows");
    }

    #[test]
    fn load_returns_error_on_invalid_packages_toml() {
        let (dir, profile, platform) =
            setup_load(linux(), &[("packages.toml", "[base\npackages = [")]);
        let result = Config::load(dir.path(), &profile, platform, None);
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
        let result = Config::load(dir.path(), &profile, platform, None);
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
        let result = Config::load(dir.path(), &profile, platform, None);
        assert!(result.is_err(), "invalid manifest.toml should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch_in_symlinks() {
        let (dir, profile, platform) = setup_load(
            linux(),
            &[("symlinks.toml", "[base]\nsymlinks = \"not-an-array\"\n")],
        );
        let result = Config::load(dir.path(), &profile, platform, None);
        assert!(
            result.is_err(),
            "type mismatch in symlinks.toml should return error"
        );
    }
}
