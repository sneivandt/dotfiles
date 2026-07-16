//! Composition of per-domain configuration handles.
//!
//! The application layer loads the aggregate [`Config`] and then splits it into
//! one typed [`ConfigHandle`] per domain slice.  Each concrete task holds a
//! clone of exactly the handle it needs, so no task depends on the aggregate
//! configuration type. During an app-owned reload the store swaps each
//! reloadable handle in place, and because tasks share those handles the update
//! is visible without rebuilding any task. Overlay scripts are the exception:
//! their dynamic task objects are created at startup, so their handle remains a
//! startup snapshot for the lifetime of the process.

use crate::app::config::Config;
use crate::domains::ai::config::copilot::CopilotSetting;
use crate::domains::editors::config::vscode_extensions::VsCodeExtension;
use crate::domains::files::config::chmod::ChmodEntry;
use crate::domains::files::config::symlinks::Symlink;
use crate::domains::git::config::git_config::GitSetting;
use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::domains::packages::config::packages::Package;
use crate::domains::repository::config::manifest::Manifest;
use crate::domains::system::config::registry::RegistryEntry;
use crate::domains::system::config::systemd_units::SystemdUnit;
use crate::infra::ConfigHandle;

/// Shared, atomically-swappable configuration split into per-domain handles.
///
/// Cloning is cheap (each field is an `Arc`-backed [`ConfigHandle`]) and all
/// clones observe the same slots.  The `aggregate` handle is held only by the
/// application's own validation tasks, which legitimately reason about the whole
/// configuration; domain tasks receive only their individual slice handles.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    /// Whole configuration, for app-owned validation tasks.
    pub aggregate: ConfigHandle<Config>,
    /// Packages to install via system package managers.
    pub packages: ConfigHandle<Vec<Package>>,
    /// Symlinks to create in the user's home directory.
    pub symlinks: ConfigHandle<Vec<Symlink>>,
    /// Windows registry entries to configure.
    pub registry: ConfigHandle<Vec<RegistryEntry>>,
    /// Systemd user units to enable.
    pub units: ConfigHandle<Vec<SystemdUnit>>,
    /// File permissions to apply (chmod).
    pub chmod: ConfigHandle<Vec<ChmodEntry>>,
    /// VS Code extensions to install.
    pub vscode_extensions: ConfigHandle<Vec<VsCodeExtension>>,
    /// Git configuration settings to apply globally.
    pub git_settings: ConfigHandle<Vec<GitSetting>>,
    /// GitHub Copilot CLI settings to converge.
    pub copilot_settings: ConfigHandle<Vec<CopilotSetting>>,
    /// Sparse checkout manifest for file exclusions.
    pub manifest: ConfigHandle<Manifest>,
    /// Startup snapshot of custom scripts from the overlay repository.
    ///
    /// Dynamic overlay tasks are built before execution starts and cannot be
    /// safely regenerated during a repository reload.
    pub scripts: ConfigHandle<Vec<ScriptEntry>>,
}

impl ConfigStore {
    /// Split an aggregate [`Config`] into per-domain handles.
    #[must_use]
    pub fn from_config(config: Config) -> Self {
        // Clone each slice before moving the whole config into the aggregate
        // handle; the element types are cheap to clone and this keeps the
        // domain handles independent of the aggregate snapshot.
        Self {
            packages: ConfigHandle::new(config.packages.clone()),
            symlinks: ConfigHandle::new(config.symlinks.clone()),
            registry: ConfigHandle::new(config.registry.clone()),
            units: ConfigHandle::new(config.units.clone()),
            chmod: ConfigHandle::new(config.chmod.clone()),
            vscode_extensions: ConfigHandle::new(config.vscode_extensions.clone()),
            git_settings: ConfigHandle::new(config.git_settings.clone()),
            copilot_settings: ConfigHandle::new(config.copilot_settings.clone()),
            manifest: ConfigHandle::new(config.manifest.clone()),
            scripts: ConfigHandle::new(config.scripts.clone()),
            aggregate: ConfigHandle::new(config),
        }
    }

    /// Replace reloadable handles from a freshly-loaded [`Config`].
    ///
    /// Each individual handle swap is atomic, but the complete store update is
    /// not one aggregate transaction. Phase ordering ensures tasks cannot read
    /// the store while this app-owned reload runs. The `scripts` handle is
    /// intentionally not replaced because dynamic overlay tasks are startup
    /// snapshots.
    pub fn reload(&self, mut config: Config) {
        // Keep the aggregate view consistent with the non-reloadable script
        // handle. Newly pulled scripts take effect on the next process run.
        config.scripts.clone_from(self.scripts.read().as_ref());
        self.packages.swap(config.packages.clone());
        self.symlinks.swap(config.symlinks.clone());
        self.registry.swap(config.registry.clone());
        self.units.swap(config.units.clone());
        self.chmod.swap(config.chmod.clone());
        self.vscode_extensions
            .swap(config.vscode_extensions.clone());
        self.git_settings.swap(config.git_settings.clone());
        self.copilot_settings.swap(config.copilot_settings.clone());
        self.manifest.swap(config.manifest.clone());
        self.aggregate.swap(config);
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "test code uses direct indexing for focused assertions"
)]
mod tests {
    use super::*;
    use crate::domains::overlay::config::scripts::ScriptEntry;
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    fn script(name: &str) -> ScriptEntry {
        ScriptEntry {
            name: name.to_string(),
            path: format!("scripts/{name}.sh"),
            description: None,
        }
    }

    #[test]
    fn reload_preserves_script_startup_snapshot_in_both_handles() {
        let mut initial = empty_config(PathBuf::from("/tmp"));
        initial.scripts = vec![script("initial")];
        let store = ConfigStore::from_config(initial);

        let mut reloaded = empty_config(PathBuf::from("/tmp"));
        reloaded.scripts = vec![script("reloaded")];
        store.reload(reloaded);

        assert_eq!(store.scripts.read()[0].name, "initial");
        assert_eq!(store.aggregate.read().scripts[0].name, "initial");
    }
}
