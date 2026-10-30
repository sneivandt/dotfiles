use anyhow::Result;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    PackageManager, PackageResource, get_installed_packages,
};
use crate::engine::{
    Context, Requirement, RequirementState, Resource as _, ResourceState, TaskStats,
};
use crate::infra::logging::OutputExt as _;

#[derive(Debug, Clone)]
pub(super) struct PackageInstallPlan {
    pub(super) missing: Vec<PackageResource>,
    already_ok: usize,
    pub(super) unavailable: Vec<(String, String)>,
}

impl PackageInstallPlan {
    pub(super) fn report_unavailable(&self, ctx: &Context) {
        for (name, reason) in &self.unavailable {
            ctx.log().warn(format!(
                "local package {name} unavailable; refusing AUR fallback: {reason}"
            ));
        }
    }

    pub(super) fn base_stats(&self) -> TaskStats {
        TaskStats::from_counts(
            0,
            u32::try_from(self.already_ok).unwrap_or(u32::MAX),
            0,
            u32::try_from(self.unavailable.len()).unwrap_or(u32::MAX),
        )
    }

    pub(super) fn preview_stats(&self) -> TaskStats {
        TaskStats::from_counts(
            u32::try_from(self.missing.len()).unwrap_or(u32::MAX),
            self.base_stats().already_ok_count(),
            0,
            self.base_stats().failed_count(),
        )
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
/// The resolved manager is passed to installation without a second lookup.
pub(super) fn native_manager_requirement() -> Requirement<'static, PackageManager> {
    Requirement::new("native-package-manager", |ctx, _| {
        let manager = if ctx.platform().is_linux() {
            PackageManager::Pacman
        } else {
            PackageManager::Winget
        };
        ctx.log().debug(format!("using {manager} package manager"));
        Ok(if ctx.which(manager.provider().name()) {
            RequirementState::Satisfied(manager)
        } else {
            RequirementState::unsatisfied(format!("{manager} not found"))
        })
    })
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
    if !ctx.which(tool) || packages.is_empty() {
        return false;
    }
    let Ok(installed) = get_installed_packages(manager, ctx.executor()) else {
        return false;
    };
    packages.iter().any(|p| !installed.contains(&p.name))
}

pub(super) fn build_install_plan(
    ctx: &Context,
    packages: &[Package],
    manager: PackageManager,
) -> Result<PackageInstallPlan> {
    ctx.trace_fmt(|| {
        format!(
            "batch-checking {} packages with a single query",
            packages.len()
        )
    });

    let installed = get_installed_packages(manager, ctx.executor())?;
    let unavailable_local = if manager == PackageManager::Paru {
        super::super::local_builds::unavailable_packages(ctx)?
    } else {
        std::collections::BTreeMap::new()
    };
    let mut unavailable = Vec::new();
    let mut missing = Vec::new();
    let mut already_ok = 0usize;

    for pkg in packages {
        // Use the package manager's host configuration, including repository
        // selection and policies such as IgnorePkg, for the whole transaction.
        let resource = PackageResource::new(pkg.name.clone(), manager, ctx.executor_arc());
        if matches!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        ) {
            ctx.debug_fmt(|| format!("ok {}", resource.description()));
            already_ok = already_ok.saturating_add(1);
        } else if let Some(reason) = unavailable_local.get(&pkg.name) {
            unavailable.push((pkg.name.clone(), reason.clone()));
        } else {
            missing.push(resource);
        }
    }

    Ok(PackageInstallPlan {
        missing,
        already_ok,
        unavailable,
    })
}
