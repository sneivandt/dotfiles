//! Configuration loading and validation for all TOML config files.
mod error;
mod preflight;
pub mod profiles;

macro_rules! config_section_inventory {
    ($apply:ident) => {
        $apply! {
            packages: Vec<crate::domains::packages::config::packages::Package> =>
                |config: &Config| Some(SectionCount::new("package", "packages", config.packages.len()));
            symlinks: Vec<crate::domains::files::config::symlinks::Symlink> =>
                |config: &Config| Some(SectionCount::new("symlink", "symlinks", config.symlinks.len()));
            validation_symlinks: Vec<crate::domains::files::config::symlinks::Symlink> =>
                |_config: &Config| None;
            registry: Vec<crate::domains::system::config::registry::RegistryEntry> =>
                |config: &Config| Some(SectionCount::new("registry entry", "registry entries", config.registry.len()));
            units: Vec<crate::domains::system::config::systemd_units::SystemdUnit> =>
                |config: &Config| Some(SectionCount::new("systemd unit", "systemd units", config.units.len()));
            chmod: Vec<crate::domains::files::config::chmod::ChmodEntry> =>
                |config: &Config| Some(SectionCount::new("chmod entry", "chmod entries", config.chmod.len()));
            validation_chmod: Vec<crate::domains::files::config::chmod::ChmodEntry> =>
                |_config: &Config| None;
            vscode_extensions: Vec<String> =>
                |config: &Config| Some(SectionCount::new("vscode extension", "vscode extensions", config.vscode_extensions.len()));
            git_settings: Vec<crate::domains::git::config::git_config::GitSetting> =>
                |config: &Config| Some(SectionCount::new("git setting", "git settings", config.git_settings.len()));
            agent_settings: Vec<crate::domains::ai::config::agent_settings::AgentSetting> =>
                |config: &Config| Some(SectionCount::new("agent setting", "agent settings", config.agent_settings.len()));
            scripts: Vec<crate::domains::overlay::config::scripts::ScriptEntry> =>
                |config: &Config| Some(SectionCount::new("overlay script", "overlay scripts", config.scripts.len()));
        }
    };
}

pub mod store;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::domains::ai::{apm, config::agent_settings};
use crate::domains::editors::config::vscode_extensions;
use crate::domains::files::config::{chmod, symlinks};
use crate::domains::git::config::git_config;
use crate::domains::overlay::config::scripts;
use crate::domains::packages::config::packages;
use crate::domains::system::config::{registry, systemd_units};
use crate::infra::config::{Diagnostic, category_matcher};
use crate::infra::platform::Platform;

pub(crate) const REQUIRED_CONFIG_FILES: &[&str] = &[
    "chmod.toml",
    "agent-settings.toml",
    "git-config.toml",
    "packages.toml",
    "profiles.toml",
    "registry.toml",
    "symlinks.toml",
    "systemd-units.toml",
    "vscode-extensions.toml",
];

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
}

impl<'a> SectionLoader<'a> {
    fn new(root: &'a Path, overlay_root: Option<&'a Path>, profile: &'a profiles::Profile) -> Self {
        Self {
            root,
            overlay_root,
            main: ConfigLoader::main(root),
            overlay: overlay_root.map(ConfigLoader::overlay),
            active: &profile.active_categories,
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
            items.extend(overlay.load_filtered(file, load, self.active)?);
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
            let mut extra = overlay.load_filtered(file, load, self.active)?;
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
            items.extend(overlay.load(file, load)?);
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
            items.extend(overlay.load_filtered(file, load, self.active)?);
        }
        Ok(items)
    }
}

/// A configured section's item count, for the verbose configuration summary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SectionCount {
    /// Label used when exactly one item is configured.
    pub(crate) singular: &'static str,
    /// Label used for every other count.
    pub(crate) plural: &'static str,
    pub(crate) count: usize,
}

impl SectionCount {
    const fn new(singular: &'static str, plural: &'static str, count: usize) -> Self {
        Self {
            singular,
            plural,
            count,
        }
    }

    /// The label that agrees in number with this section's count.
    pub(crate) const fn label(&self) -> &'static str {
        if self.count == 1 {
            self.singular
        } else {
            self.plural
        }
    }
}

/// All loaded configuration for a resolved profile.
#[derive(Debug, Clone)]
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
    /// Main and overlay symlink definitions before category filtering.
    /// Used by repository validation to inspect every declared source.
    pub validation_symlinks: Vec<symlinks::Symlink>,
    /// Windows registry entries to configure.
    pub registry: Vec<registry::RegistryEntry>,
    /// Systemd user units to enable.
    pub units: Vec<systemd_units::SystemdUnit>,
    /// File permissions to apply (chmod).
    pub chmod: Vec<chmod::ChmodEntry>,
    /// Main and overlay chmod definitions before category filtering.
    pub validation_chmod: Vec<chmod::ChmodEntry>,
    /// VS Code extensions to install.
    pub vscode_extensions: Vec<String>,
    /// Git configuration settings to apply globally.
    pub git_settings: Vec<git_config::GitSetting>,
    /// User settings to converge for supported agent harnesses.
    pub agent_settings: Vec<agent_settings::AgentSetting>,
    /// Custom scripts from the overlay repository.
    pub scripts: Vec<scripts::ScriptEntry>,
}

impl Config {
    /// Load all configuration for the given profile from the conf/ directory,
    /// optionally merging additional configuration from an overlay repository.
    ///
    /// # Errors
    ///
    /// Returns an error if a configuration file cannot be parsed, symlink targets
    /// conflict, or active Git/registry declarations specify contradictory values.
    pub fn load(
        root: &Path,
        profile: &profiles::Profile,
        platform: Platform,
        overlay: Option<&Path>,
    ) -> Result<Self> {
        let configured_categories = profiles::configured_categories(&root.join("conf"))?;
        preflight::validate(root, overlay, &configured_categories)?;
        let sections = SectionLoader::new(root, overlay, profile);

        // Each field is loaded and overlay-merged by a single `SectionLoader`
        // call, so adding a new config section means adding one struct field
        // and one line here — never a second edit in a separate merge step.
        let mut validation_symlinks = sections
            .main
            .load(symlinks::SYMLINKS_TOML, symlinks::load_all)?;
        symlinks::set_origin(&mut validation_symlinks, root);
        if let (Some(overlay_loader), Some(overlay_root)) =
            (&sections.overlay, sections.overlay_root)
        {
            let mut overlay_symlinks =
                overlay_loader.load(symlinks::SYMLINKS_TOML, symlinks::load_all)?;
            symlinks::set_origin(&mut overlay_symlinks, overlay_root);
            validation_symlinks.extend(overlay_symlinks);
        }
        let validation_chmod = sections.collect_unfiltered(chmod::CHMOD_TOML, chmod::load_all)?;
        let registry = sections.collect_unfiltered(registry::REGISTRY_TOML, registry::load)?;
        let units =
            sections.collect_filtered(systemd_units::SYSTEMD_UNITS_TOML, systemd_units::load)?;
        let mut config = Self {
            root: root.to_path_buf(),
            overlay: overlay.map(Path::to_path_buf),
            profile: profile.clone(),
            packages: sections.collect_filtered(packages::PACKAGES_TOML, packages::load)?,
            symlinks: sections.collect_filtered_post(
                symlinks::SYMLINKS_TOML,
                symlinks::load,
                symlinks::set_origin,
            )?,
            validation_symlinks,
            registry: if platform.has_registry() {
                registry
            } else {
                Vec::new()
            },
            units: if platform.supports_systemd() {
                units
            } else {
                Vec::new()
            },
            chmod: sections.collect_filtered(chmod::CHMOD_TOML, chmod::load)?,
            validation_chmod,
            vscode_extensions: sections.collect_filtered(
                vscode_extensions::VSCODE_EXTENSIONS_TOML,
                vscode_extensions::load,
            )?,
            git_settings: sections
                .collect_filtered(git_config::GIT_CONFIG_TOML, git_config::load)?,
            agent_settings: sections
                .collect_filtered(agent_settings::AGENT_SETTINGS_TOML, agent_settings::load)?,
            scripts: sections.collect_overlay_only(scripts::SCRIPTS_TOML, scripts::load)?,
        };

        config.symlinks = symlinks::expand_glob_patterns(&config.symlinks, root)
            .context("expanding symlink glob patterns")?;
        symlinks::validate_unique_targets(&config.symlinks)
            .context("validating symlink targets")?;

        let conflicts = git_config::validate_conflicts(&config.git_settings)
            .into_iter()
            .chain(registry::validate_conflicts(&config.registry))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            conflicts.is_empty(),
            "contradictory desired state:\n{}",
            conflicts
                .iter()
                .map(|diagnostic| format!(
                    "  {} [{}] ({}): {}",
                    diagnostic.source, diagnostic.item, diagnostic.code, diagnostic.message
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );

        Ok(config)
    }

    /// Validate the configuration and return any diagnostics.
    ///
    /// This method checks for common configuration issues such as:
    /// - Missing source files for symlinks
    /// - Invalid values (e.g., invalid octal modes for chmod)
    /// - Platform incompatibilities
    #[must_use]
    pub fn validate(&self, platform: Platform) -> Vec<Diagnostic> {
        let mut diagnostics = symlinks::validate(&self.symlinks, &self.root);
        diagnostics.extend(apm::validate(&self.root, self.overlay.as_deref()));
        diagnostics.extend(packages::validate(&self.packages, platform));
        diagnostics.extend(registry::validate(&self.registry, platform));
        diagnostics.extend(chmod::validate(&self.chmod, platform));
        diagnostics.extend(systemd_units::validate(&self.units, platform));
        diagnostics.extend(vscode_extensions::validate(&self.vscode_extensions));
        diagnostics.extend(git_config::validate(&self.git_settings));
        diagnostics.extend(agent_settings::validate(&self.agent_settings));
        diagnostics.extend(scripts::validate(&self.scripts));
        diagnostics
    }

    /// Return configured item counts for debug logging.
    #[must_use]
    pub(crate) fn section_counts(&self) -> Vec<SectionCount> {
        macro_rules! collect_section_counts {
            ($($field:ident: $ty:ty => $count:expr;)+) => {
                [$(($count)(self)),+].into_iter().flatten().collect()
            };
        }

        config_section_inventory!(collect_section_counts)
    }
}

#[cfg(test)]
mod tests;
