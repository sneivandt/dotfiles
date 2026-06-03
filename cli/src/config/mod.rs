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

pub(crate) use helpers::section_macro::config_section;

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

/// Loads config sections from the main `conf/` directory and, when present,
/// merges matching sections from an overlay repository.
///
/// Each `collect_*` method performs the main load and the overlay merge for a
/// single section in one call.  Keeping both halves in one place makes it
/// structurally impossible for a section to be loaded without also being
/// merged from the overlay — the desync footgun that a hand-written
/// `load` + `merge_overlay` pair invited.
struct SectionLoader<'a> {
    root: &'a Path,
    overlay_root: Option<&'a Path>,
    main: ConfigLoader,
    overlay: Option<ConfigLoader>,
    active: &'a [category_matcher::Category],
    excluded: &'a [category_matcher::Category],
}

impl<'a> SectionLoader<'a> {
    fn new(root: &'a Path, overlay_root: Option<&'a Path>, profile: &'a profiles::Profile) -> Self {
        Self {
            root,
            overlay_root,
            main: ConfigLoader::main(root),
            overlay: overlay_root.map(ConfigLoader::overlay),
            active: &profile.active_categories,
            excluded: &profile.excluded_categories,
        }
    }

    /// Load a category-filtered section from main config and append the
    /// overlay's matching section.
    fn collect_filtered<T>(
        &self,
        file: &str,
        load: fn(&Path, &[category_matcher::Category]) -> Result<Vec<T>>,
    ) -> Result<Vec<T>> {
        let mut items = self.main.load_filtered(file, load, self.active)?;
        if let Some(overlay) = &self.overlay {
            items.extend(overlay.load_overlay_filtered(file, load, self.active)?);
        }
        Ok(items)
    }

    /// Like [`collect_filtered`](Self::collect_filtered) but applies `post` to
    /// each batch using its originating root, so main and overlay items keep
    /// the correct provenance (used by symlinks to set their origin).
    fn collect_filtered_post<T>(
        &self,
        file: &str,
        load: fn(&Path, &[category_matcher::Category]) -> Result<Vec<T>>,
        post: impl Fn(&mut [T], &Path),
    ) -> Result<Vec<T>> {
        let mut items = self.main.load_filtered(file, load, self.active)?;
        post(&mut items, self.root);
        if let (Some(overlay), Some(overlay_root)) = (&self.overlay, self.overlay_root) {
            let mut extra = overlay.load_overlay_filtered(file, load, self.active)?;
            post(&mut extra, overlay_root);
            items.extend(extra);
        }
        Ok(items)
    }

    /// Load an unfiltered section (no category tags) from main config and
    /// append the overlay's matching section.
    fn collect_unfiltered<T>(
        &self,
        file: &str,
        load: fn(&Path) -> Result<Vec<T>>,
    ) -> Result<Vec<T>> {
        let mut items = self.main.load(file, load)?;
        if let Some(overlay) = &self.overlay {
            items.extend(overlay.load_overlay(file, load)?);
        }
        Ok(items)
    }

    /// Collect a category-filtered section from the overlay only; the main
    /// `conf/` directory does not provide this section.
    fn collect_overlay_only<T>(
        &self,
        file: &str,
        load: fn(&Path, &[category_matcher::Category]) -> Result<Vec<T>>,
    ) -> Result<Vec<T>> {
        let mut items = Vec::new();
        if let Some(overlay) = &self.overlay {
            items.extend(overlay.load_overlay_filtered(file, load, self.active)?);
        }
        Ok(items)
    }

    /// Load a single-value section filtered by the profile's *excluded*
    /// categories.  Not merged from the overlay.
    fn load_excluded<T>(
        &self,
        file: &str,
        load: fn(&Path, &[category_matcher::Category]) -> Result<T>,
    ) -> Result<T> {
        self.main.load_filtered(file, load, self.excluded)
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
        let sections = SectionLoader::new(root, overlay, profile);

        // Each field is loaded and overlay-merged by a single `SectionLoader`
        // call, so adding a new config section means adding one struct field
        // and one line here — never a second edit in a separate merge step.
        let mut config = Self {
            root: root.to_path_buf(),
            overlay: overlay.map(Path::to_path_buf),
            profile: profile.clone(),
            packages: sections.collect_filtered(PACKAGES_TOML, packages::load)?,
            symlinks: sections.collect_filtered_post(
                SYMLINKS_TOML,
                symlinks::load,
                symlinks::set_origin,
            )?,
            registry: if platform.has_registry() {
                sections.collect_unfiltered(REGISTRY_TOML, registry::load)?
            } else {
                Vec::new()
            },
            units: if platform.supports_systemd() {
                sections.collect_filtered(SYSTEMD_UNITS_TOML, systemd_units::load)?
            } else {
                Vec::new()
            },
            chmod: sections.collect_filtered(CHMOD_TOML, chmod::load)?,
            vscode_extensions: sections
                .collect_filtered(VSCODE_EXTENSIONS_TOML, vscode_extensions::load)?,
            git_settings: sections.collect_filtered(GIT_CONFIG_TOML, git_config::load)?,
            copilot_settings: sections.collect_filtered(COPILOT_TOML, copilot::load)?,
            manifest: sections.load_excluded(MANIFEST_TOML, manifest::load)?,
            scripts: sections.collect_overlay_only(SCRIPTS_TOML, scripts::load)?,
        };

        config.symlinks = symlinks::expand_glob_patterns(&config.symlinks, root)
            .context("expanding symlink glob patterns")?;

        Ok(config)
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
mod tests;
