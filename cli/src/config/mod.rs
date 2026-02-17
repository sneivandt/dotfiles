pub mod chmod;
pub mod copilot_skills;
pub mod ini;
pub mod manifest;
pub mod packages;
pub mod profiles;
pub mod registry;
pub mod symlinks;
pub mod units;
pub mod vscode;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::platform::Platform;

/// All loaded configuration for a resolved profile.
#[derive(Debug)]
pub struct Config {
    pub root: PathBuf,
    pub packages: Vec<packages::Package>,
    pub symlinks: Vec<symlinks::Symlink>,
    pub registry: Vec<registry::RegistryEntry>,
    pub units: Vec<units::Unit>,
    pub chmod: Vec<chmod::ChmodEntry>,
    pub vscode_extensions: Vec<vscode::VsCodeExtension>,
    pub copilot_skills: Vec<copilot_skills::CopilotSkill>,
    pub manifest: manifest::Manifest,
}

impl Config {
    /// Load all configuration for the given profile from the conf/ directory.
    pub fn load(root: &Path, profile: &profiles::Profile, platform: &Platform) -> Result<Self> {
        let conf = root.join("conf");

        let active_categories = &profile.active_categories;
        let excluded_categories = &profile.excluded_categories;

        let packages = packages::load(&conf.join("packages.ini"), active_categories)
            .context("loading packages.ini")?;

        let symlinks = symlinks::load(&conf.join("symlinks.ini"), active_categories)
            .context("loading symlinks.ini")?;

        let registry = if platform.is_windows() {
            registry::load(&conf.join("registry.ini"), active_categories)
                .context("loading registry.ini")?
        } else {
            Vec::new()
        };

        let units = if platform.is_linux() {
            units::load(&conf.join("units.ini"), active_categories).context("loading units.ini")?
        } else {
            Vec::new()
        };

        let chmod_entries =
            chmod::load(&conf.join("chmod.ini"), active_categories).context("loading chmod.ini")?;

        let vscode_extensions =
            vscode::load(&conf.join("vscode-extensions.ini"), active_categories)
                .context("loading vscode-extensions.ini")?;

        let copilot_skills =
            copilot_skills::load(&conf.join("copilot-skills.ini"), active_categories)
                .context("loading copilot-skills.ini")?;

        let manifest = manifest::load(&conf.join("manifest.ini"), excluded_categories)
            .context("loading manifest.ini")?;

        Ok(Self {
            root: root.to_path_buf(),
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
}
