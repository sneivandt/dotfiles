//! Installation strategies for different package managers.
use anyhow::Result;
use std::sync::Arc;

use crate::config::packages::Package;
use crate::resources::Applicable as _;
use crate::resources::package::{
    PackageManager, PackageResource, batch_install_packages, get_installed_packages,
};
use crate::tasks::{Context, ProcessOpts, TaskResult, TaskStats, process_resource_states};

/// Strategy for installing packages within a single package-manager scope.
///
/// Implementations decide **how** missing packages are installed (one-by-one
/// vs. a single batch command) while [`process_packages`] handles the shared
/// query-installed-first-then-delegate workflow.
pub(super) trait PackageStrategy {
    /// Install the given packages, using `installed` to skip already-present
    /// packages.
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &std::collections::HashSet<String>,
    ) -> Result<TaskResult>;
}

/// Batch strategy (Pacman / Paru): collect all missing packages and install
/// them in **one** package-manager invocation.  This is faster and lets the
/// solver resolve cross-package dependencies across the full set.
struct BatchInstall {
    manager: PackageManager,
}

impl PackageStrategy for BatchInstall {
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &std::collections::HashSet<String>,
    ) -> Result<TaskResult> {
        let resources: Vec<PackageResource> = packages
            .iter()
            .map(|pkg| {
                PackageResource::new(pkg.name.clone(), self.manager, Arc::clone(&ctx.executor))
            })
            .collect();

        let mut stats = TaskStats::new();
        let mut missing = Vec::new();

        for r in &resources {
            if installed.contains(&r.name) {
                ctx.log.debug(&format!("ok: {}", r.description()));
                stats.already_ok += 1;
            } else {
                missing.push(r);
            }
        }

        if missing.is_empty() {
            return Ok(stats.finish(ctx));
        }

        if ctx.dry_run {
            for r in &missing {
                ctx.log
                    .dry_run(&format!("would install: {}", r.description()));
            }
            stats.changed = missing.len() as u32;
            return Ok(stats.finish(ctx));
        }

        ctx.log
            .debug(&format!("batch-installing {} packages", missing.len()));
        if let Err(e) = batch_install_packages(&missing) {
            ctx.log.warn(&format!("batch install failed: {e:#}"));
            stats.skipped = missing.len() as u32;
        } else {
            stats.changed = missing.len() as u32;
        }

        Ok(stats.finish(ctx))
    }
}

/// Individual strategy (Winget): install each package separately so that one
/// failure does not prevent the remainder from being attempted.
struct IndividualInstall {
    manager: PackageManager,
}

impl PackageStrategy for IndividualInstall {
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &std::collections::HashSet<String>,
    ) -> Result<TaskResult> {
        let resource_states = packages.iter().map(|pkg| {
            let resource =
                PackageResource::new(pkg.name.clone(), self.manager, Arc::clone(&ctx.executor));
            let state = resource.state_from_installed(installed);
            (resource, state)
        });
        process_resource_states(ctx, resource_states, &ProcessOpts::lenient("install"))
    }
}

/// Process a list of packages using the appropriate strategy for the given
/// package manager.
///
/// Queries all installed packages **once**, then delegates to either
/// [`BatchInstall`] (Pacman / Paru) or [`IndividualInstall`] (Winget).
pub(super) fn process_packages(
    ctx: &Context,
    packages: &[Package],
    manager: PackageManager,
) -> Result<TaskResult> {
    ctx.log.debug(&format!(
        "batch-checking {} packages with a single query",
        packages.len()
    ));
    let installed = get_installed_packages(manager, &*ctx.executor)?;

    let strategy: &dyn PackageStrategy = match manager {
        PackageManager::Winget => &IndividualInstall { manager },
        _ => &BatchInstall { manager },
    };
    strategy.install(ctx, packages, &installed)
}
