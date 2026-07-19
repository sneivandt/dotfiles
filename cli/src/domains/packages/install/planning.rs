use anyhow::Result;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    PackageManager, PackageResource, get_installed_packages,
};
use crate::engine::{Context, Resource as _, ResourceState, TaskStats};

#[derive(Debug, Clone)]
pub(super) struct PackageInstallPlan {
    pub(super) missing: Vec<PackageResource>,
    already_ok: usize,
}

impl PackageInstallPlan {
    pub(super) fn base_stats(&self) -> TaskStats {
        let mut stats = TaskStats::new();
        stats.already_ok = u32::try_from(self.already_ok).unwrap_or(u32::MAX);
        stats
    }

    pub(super) fn preview_stats(&self) -> TaskStats {
        let mut stats = self.base_stats();
        stats.changed = u32::try_from(self.missing.len()).unwrap_or(u32::MAX);
        stats
    }
}

/// Collect packages matching the AUR/native filter from a package slice.
pub(super) fn select_packages(packages: &[Package], is_aur: bool) -> Vec<Package> {
    packages
        .iter()
        .filter(|p| p.is_aur == is_aur)
        .cloned()
        .collect()
}

/// Resolve the package manager for native (non-AUR) installs based on platform
/// and tool availability.
///
/// Returns `Ok(manager)` when one is usable, or `Err(reason)` describing why
/// the task should skip.
pub(super) fn resolve_native_manager(ctx: &Context) -> Result<PackageManager, String> {
    let system = ctx.system();
    if system.platform().is_linux() {
        ctx.log().debug("using pacman package manager");
        if !system.which("pacman") {
            return Err("pacman not found".to_string());
        }
        Ok(PackageManager::Pacman)
    } else {
        ctx.log().debug("using winget package manager");
        if !system.which("winget") {
            return Err("winget not found".to_string());
        }
        Ok(PackageManager::Winget)
    }
}

/// Predict whether an install of `packages` via `manager` will require sudo.
///
/// Returns `false` (no sudo prompt) when:
/// - the manager tool is missing,
/// - the package list is empty, or
/// - the installed-packages query fails (we cannot prove anything is missing).
///
/// Otherwise returns `true` iff at least one configured package is not yet
/// installed — i.e. a sudo-using install command will actually run.
pub(super) fn predict_sudo(
    ctx: &Context,
    manager: PackageManager,
    tool: &str,
    packages: &[Package],
) -> bool {
    let system = ctx.system();
    if !system.which(tool) || packages.is_empty() {
        return false;
    }
    let Ok(installed) = get_installed_packages(manager, system.executor()) else {
        return false;
    };
    packages.iter().any(|p| !installed.contains(&p.name))
}

pub(super) fn build_install_plan(
    ctx: &Context,
    packages: &[Package],
    manager: PackageManager,
) -> Result<PackageInstallPlan> {
    ctx.debug_fmt(|| {
        format!(
            "batch-checking {} packages with a single query",
            packages.len()
        )
    });

    let system = ctx.system();
    let installed = get_installed_packages(manager, system.executor())?;
    let resources: Vec<PackageResource> = packages
        .iter()
        .map(|pkg| PackageResource::new(pkg.name.clone(), manager, system.executor_arc()))
        .collect();
    let mut missing = Vec::new();
    let mut already_ok = 0usize;

    for resource in resources {
        if matches!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        ) {
            ctx.debug_fmt(|| format!("ok: {}", resource.description()));
            already_ok = already_ok.saturating_add(1);
        } else {
            missing.push(resource);
        }
    }

    Ok(PackageInstallPlan {
        missing,
        already_ok,
    })
}
