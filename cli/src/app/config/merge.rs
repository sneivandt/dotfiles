//! Resource-aware main/overlay precedence without changing within-source semantics.

use std::path::{Path, PathBuf};

use crate::infra::platform::{Os, Platform};
use anyhow::{Context as _, Result};

use super::{agent_settings, chmod, error, git_config, packages, registry, symlinks};
use super::{system_files, systemd_units};

pub(super) trait OverlayEntry: Sized {
    fn same_target(&self, other: &Self, platform: Platform) -> bool;

    fn prepare(entries: Vec<Self>, _root: &Path) -> Result<Vec<Self>> {
        Ok(entries)
    }
}

pub(super) fn overlay<T: OverlayEntry>(
    mut main: Vec<T>,
    overlay: Vec<T>,
    platform: Platform,
) -> Vec<T> {
    main.retain(|entry| {
        !overlay
            .iter()
            .any(|replacement| entry.same_target(replacement, platform))
    });
    main.extend(overlay);
    main
}

impl OverlayEntry for packages::Package {
    fn same_target(&self, other: &Self, platform: Platform) -> bool {
        if platform.os == Os::Windows {
            self.name.eq_ignore_ascii_case(&other.name)
        } else {
            self.name == other.name
        }
    }
}

impl OverlayEntry for String {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        self.eq_ignore_ascii_case(other)
    }
}

impl OverlayEntry for systemd_units::SystemdUnit {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        self.name == other.name && self.scope == other.scope
    }
}

impl OverlayEntry for chmod::ChmodEntry {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        normalized_path(&self.path) == normalized_path(&other.path)
    }
}

fn normalized_path(path: &str) -> PathBuf {
    Path::new(path)
        .components()
        .filter(|component| *component != std::path::Component::CurDir)
        .collect()
}

impl OverlayEntry for git_config::GitSetting {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        git_config::target_key(&self.key) == git_config::target_key(&other.key)
    }

    fn prepare(entries: Vec<Self>, _root: &Path) -> Result<Vec<Self>> {
        error::reject_conflicts(git_config::validate_conflicts(&entries))?;
        Ok(entries)
    }
}

impl OverlayEntry for registry::RegistryEntry {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        registry::target_key(self) == registry::target_key(other)
    }

    fn prepare(entries: Vec<Self>, _root: &Path) -> Result<Vec<Self>> {
        error::reject_conflicts(registry::validate_conflicts(&entries))?;
        Ok(entries)
    }
}

impl OverlayEntry for agent_settings::AgentSetting {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        self.target == other.target && self.key == other.key
    }

    fn prepare(entries: Vec<Self>, _root: &Path) -> Result<Vec<Self>> {
        error::reject_conflicts(agent_settings::validate_conflicts(&entries))?;
        Ok(entries)
    }
}

impl OverlayEntry for symlinks::Symlink {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        symlinks::target_identity(self) == symlinks::target_identity(other)
    }

    fn prepare(entries: Vec<Self>, root: &Path) -> Result<Vec<Self>> {
        let entries = symlinks::expand_glob_patterns(&entries, root)
            .context("expanding symlink glob patterns")?;
        symlinks::validate_unique_targets(&entries).context("validating symlink targets")?;
        Ok(entries)
    }
}

impl OverlayEntry for system_files::SystemFile {
    fn same_target(&self, other: &Self, _platform: Platform) -> bool {
        self.target == other.target
    }

    fn prepare(entries: Vec<Self>, _root: &Path) -> Result<Vec<Self>> {
        system_files::validate_entries(&entries).context("validating system file targets")?;
        Ok(entries)
    }
}
