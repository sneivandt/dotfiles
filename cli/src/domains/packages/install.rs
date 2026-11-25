//! Tasks: install system packages.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    InstalledPackages, PackageManager, PackageResource, get_installed_packages,
    install_missing_packages,
};
use crate::engine::Resource as _;
use crate::engine::{
    BatchCompletion, BatchReport, Context, Operation, OperationState, Requirement,
    RequirementState, Task, TaskResult, TaskStats, process_operation, process_requirement,
    task_metadata,
};
use crate::infra::ConfigHandle;
use crate::infra::exec::{ExecError, Executor};
use crate::infra::logging::OutputExt as _;

mod paru;
mod planning;

use paru::{ParuHealth, build_paru, check_paru_health, check_prerequisites, clone_paru_from_aur};
use planning::{
    PackageInstallPlan, build_install_plan, native_manager_requirement, predict_sudo,
    select_packages,
};

/// Execution phase owning a cached `pacman -Q` snapshot.
///
/// Assessment probes share one snapshot; entering task execution refreshes
/// once so external package changes made between assessment and execution are
/// observed while sibling execution tasks still share a single query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InventoryPhase {
    /// Pre-execution elevation and ordering-requirement probes.
    Assessment,
    /// Task execution planning (`InstallPackages` / `InstallAurPackages`).
    Execution,
}

/// Shared per-run cache for `pacman -Q` installed-package snapshots.
///
/// Shared across `InstallPackages`, `InstallParu`, `InstallAurPackages`, and
/// package-stability requirements within one command run. Assessment probes
/// share one snapshot; entering execution refreshes once so external changes
/// made after assessment are visible, and any package mutation invalidates
/// the snapshot immediately before running.
#[derive(Debug, Default, Clone)]
pub(crate) struct PacmanInventoryCache(Arc<Mutex<Option<(InventoryPhase, InstalledPackages)>>>);

impl PacmanInventoryCache {
    /// Create an empty cache for a single command run.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Return the cached `pacman -Q` snapshot for `phase`, querying once on a
    /// miss or phase transition without holding the mutex across subprocess
    /// execution.
    pub(super) fn query(
        &self,
        phase: InventoryPhase,
        manager: PackageManager,
        executor: &dyn Executor,
    ) -> Result<InstalledPackages> {
        if !matches!(manager, PackageManager::Pacman | PackageManager::Paru) {
            return get_installed_packages(manager, executor);
        }
        let cached = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(cached_phase, _)| *cached_phase == phase)
            .map(|(_, installed)| installed.clone());
        if let Some(cached) = cached {
            return Ok(cached);
        }
        let queried = get_installed_packages(PackageManager::Pacman, executor)?;
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((phase, queried.clone()));
        Ok(queried)
    }

    /// Drop any cached snapshot so the next query reads live pacman state.
    pub(crate) fn invalidate(&self) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

pub(super) fn query_installed_with_cache(
    cache: Option<&PacmanInventoryCache>,
    phase: InventoryPhase,
    manager: PackageManager,
    executor: &dyn Executor,
) -> Result<InstalledPackages> {
    cache.map_or_else(
        || get_installed_packages(manager, executor),
        |cache| cache.query(phase, manager, executor),
    )
}

/// Prove selected package work cannot invalidate a consumer's observed inputs.
#[cfg(test)]
pub(crate) fn package_stability_requirement(
    packages: ConfigHandle<Vec<Package>>,
) -> Requirement<'static> {
    package_stability_requirement_with_cache(packages, None)
}

/// Prove selected package work cannot invalidate a consumer's observed inputs,
/// reusing a shared per-run pacman inventory cache when provided.
pub(crate) fn package_stability_requirement_with_cache(
    packages: ConfigHandle<Vec<Package>>,
    inventory_cache: Option<PacmanInventoryCache>,
) -> Requirement<'static> {
    Requirement::new("package-inputs-stable", move |ctx, _| {
        check_package_stability(ctx, packages.get(), inventory_cache.as_ref())
    })
}

fn check_package_stability(
    ctx: &Context,
    packages: &[Package],
    inventory_cache: Option<&PacmanInventoryCache>,
) -> Result<RequirementState<()>> {
    if !ctx.platform().uses_pacman() || !ctx.which("pacman") {
        return Ok(RequirementState::unsatisfied(
            "package stability requires a supported pacman inventory",
        ));
    }
    let installed = query_installed_with_cache(
        inventory_cache,
        InventoryPhase::Assessment,
        PackageManager::Pacman,
        ctx.executor(),
    )?;
    if packages
        .iter()
        .any(|package| !installed.contains_package(&package.name))
    {
        return Ok(RequirementState::unsatisfied(
            "configured packages still need installation",
        ));
    }
    Ok(match check_paru_health(ctx.executor())? {
        ParuHealth::Healthy { .. } => RequirementState::Satisfied(()),
        ParuHealth::Missing { reason } | ParuHealth::Broken { reason, .. } => {
            RequirementState::unsatisfied(reason)
        }
    })
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Install system packages via pacman or winget.
#[derive(Debug)]
pub struct InstallPackages {
    config: ConfigHandle<Vec<Package>>,
    inventory_cache: Option<PacmanInventoryCache>,
}

impl InstallPackages {
    /// Construct this task with its configuration handle.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<Package>>) -> Self {
        Self {
            config,
            inventory_cache: None,
        }
    }

    /// Share a per-run pacman inventory cache with sibling package tasks.
    #[must_use]
    pub(crate) fn with_inventory_cache(mut self, cache: PacmanInventoryCache) -> Self {
        self.inventory_cache = Some(cache);
        self
    }
}

impl Task for InstallPackages {
    task_metadata! {
        name: "System packages",
        selector: "packages",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        PackageTaskKind::Native.should_run(ctx, self.config.get())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        PackageTaskKind::Native.needs_elevation(
            ctx,
            self.config.get(),
            self.inventory_cache.as_ref(),
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Native.run(ctx, self.config.get(), self.inventory_cache.as_ref())
    }
}

/// Install AUR packages via paru.
#[derive(Debug)]
pub struct InstallAurPackages {
    config: ConfigHandle<Vec<Package>>,
    inventory_cache: Option<PacmanInventoryCache>,
}

impl InstallAurPackages {
    /// Construct this task with its configuration handle.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<Package>>) -> Self {
        Self {
            config,
            inventory_cache: None,
        }
    }

    /// Share a per-run pacman inventory cache with sibling package tasks.
    #[must_use]
    pub(crate) fn with_inventory_cache(mut self, cache: PacmanInventoryCache) -> Self {
        self.inventory_cache = Some(cache);
        self
    }
}

#[derive(Clone, Copy)]
enum PackageTaskKind {
    Native,
    Aur,
}

impl PackageTaskKind {
    const fn is_aur(self) -> bool {
        matches!(self, Self::Aur)
    }

    fn select(self, packages: &[Package]) -> Vec<Package> {
        select_packages(packages, self.is_aur())
    }

    fn should_run(self, ctx: &Context, packages: &[Package]) -> bool {
        let platform_is_supported = match self {
            Self::Native => true,
            Self::Aur => ctx.platform().supports_aur(),
        };
        platform_is_supported
            && packages
                .iter()
                .any(|package| package.is_aur == self.is_aur())
    }

    fn needs_elevation(
        self,
        ctx: &Context,
        packages: &[Package],
        inventory_cache: Option<&PacmanInventoryCache>,
    ) -> bool {
        let platform = ctx.platform();
        let (supported, manager, executable) = match self {
            Self::Native => (platform.uses_pacman(), PackageManager::Pacman, "pacman"),
            Self::Aur => (platform.supports_aur(), PackageManager::Paru, "paru"),
        };
        supported
            && predict_sudo(
                ctx,
                manager,
                executable,
                &self.select(packages),
                inventory_cache,
            )
    }

    fn run(
        self,
        ctx: &Context,
        packages: &[Package],
        inventory_cache: Option<&PacmanInventoryCache>,
    ) -> Result<TaskResult> {
        let selected = self.select(packages);
        if selected.is_empty() {
            let reason = match self {
                Self::Native => "no packages to install",
                Self::Aur => "no AUR packages",
            };
            return Ok(TaskResult::NotApplicable(reason.to_string()));
        }

        let manager = match self {
            Self::Native => {
                ctx.trace_fmt(|| format!("{} non-AUR packages to process", selected.len()));
                return process_requirement(ctx, &native_manager_requirement(), |manager| {
                    process_operation(
                        ctx,
                        &PackageInstallOperation::new(selected, manager)
                            .with_inventory_cache(inventory_cache.cloned()),
                    )
                });
            }
            // A preview queries pacman's database but must not require the helper
            // that the preceding bootstrap task only *planned* to install.
            Self::Aur if ctx.dry_run() => PackageManager::Paru,
            Self::Aur => {
                let path = match check_paru_health(ctx.executor())? {
                    ParuHealth::Healthy { path, .. } => path,
                    ParuHealth::Missing { reason } => anyhow::bail!(
                        "paru became unavailable after bootstrap validation: {reason}"
                    ),
                    ParuHealth::Broken { path, reason } => anyhow::bail!(
                        "PATH-selected paru executable {} failed after bootstrap validation: {reason}",
                        path.display()
                    ),
                };
                ctx.debug_fmt(|| format!("using validated paru executable: {}", path.display()));
                ctx.debug_fmt(|| format!("checking {} AUR packages", selected.len()));
                PackageManager::Paru
            }
        };

        process_operation(
            ctx,
            &PackageInstallOperation::new(selected, manager)
                .with_inventory_cache(inventory_cache.cloned()),
        )
    }
}

impl Task for InstallAurPackages {
    task_metadata! {
        name: "AUR packages",
        selector: "aur-packages",
        deps: [
            InstallParu,
        ],
    }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                super::arch_recipes::ConfigureParu,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        PackageTaskKind::Aur.should_run(ctx, self.config.get())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        PackageTaskKind::Aur.needs_elevation(ctx, self.config.get(), self.inventory_cache.as_ref())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Aur.run(ctx, self.config.get(), self.inventory_cache.as_ref())
    }
}

/// Install paru AUR helper.
#[derive(Debug, Default)]
pub(crate) struct InstallParu {
    inventory_cache: Option<PacmanInventoryCache>,
}

impl InstallParu {
    /// Construct the paru installation task.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            inventory_cache: None,
        }
    }

    /// Share a per-run pacman inventory cache so paru builds invalidate it.
    #[must_use]
    pub(crate) fn with_inventory_cache(mut self, cache: PacmanInventoryCache) -> Self {
        self.inventory_cache = Some(cache);
        self
    }
}

#[cfg(test)]
fn run_paru_install(ctx: &Context, build_root: &Path) -> Result<TaskResult> {
    run_paru_install_with_cache(ctx, build_root, None)
}

fn run_paru_install_with_cache(
    ctx: &Context,
    build_root: &Path,
    inventory_cache: Option<&PacmanInventoryCache>,
) -> Result<TaskResult> {
    process_operation(
        ctx,
        &ParuInstallOperation {
            build_root,
            inventory_cache,
        },
    )
}

impl Task for InstallParu {
    task_metadata! {
        name: "Paru package manager",
        selector: "paru",
    }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                InstallPackages,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().uses_pacman()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        // makepkg -si calls sudo internally to install the built package
        ctx.platform().uses_pacman()
            && matches!(
                check_paru_health(ctx.executor()),
                Ok(ParuHealth::Missing { .. } | ParuHealth::Broken { .. })
            )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        run_paru_install_with_cache(ctx, &std::env::temp_dir(), self.inventory_cache.as_ref())
    }
}

#[derive(Debug, Clone, Copy)]
struct ParuInstallOperation<'a> {
    build_root: &'a Path,
    inventory_cache: Option<&'a PacmanInventoryCache>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParuInstallPlan {
    Install {
        reason: String,
    },
    Rebuild {
        path: std::path::PathBuf,
        reason: String,
    },
}

impl ParuInstallPlan {
    const fn action(&self) -> &'static str {
        match self {
            Self::Install { .. } => "install",
            Self::Rebuild { .. } => "rebuild",
        }
    }

    const fn completed_action(&self) -> &'static str {
        match self {
            Self::Install { .. } => "installed",
            Self::Rebuild { .. } => "rebuilt",
        }
    }
}

impl Operation for ParuInstallOperation<'_> {
    type Plan = ParuInstallPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        match check_paru_health(ctx.executor())? {
            ParuHealth::Missing { reason } => {
                ctx.log().debug(format!("paru status: missing · {reason}"));
                Ok(OperationState::needs_run(ParuInstallPlan::Install {
                    reason,
                }))
            }
            ParuHealth::Healthy {
                path,
                package,
                version,
            } => {
                ctx.log().debug(format!(
                    "paru status: healthy · target package {package} · executable {} · {version}",
                    path.display()
                ));
                Ok(OperationState::Complete)
            }
            ParuHealth::Broken { path, reason } => {
                ctx.log().warn(format!(
                    "paru status: broken · executable {} · {reason}",
                    path.display()
                ));
                Ok(OperationState::needs_run(ParuInstallPlan::Rebuild {
                    path,
                    reason,
                }))
            }
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        match plan {
            ParuInstallPlan::Install { reason } => ctx.log().action(
                "install",
                "missing paru from AUR source",
                true,
                &format!("install missing paru from AUR source · {reason}"),
            ),
            ParuInstallPlan::Rebuild { path, reason } => ctx.log().action(
                "rebuild",
                "broken paru from AUR source",
                true,
                &format!(
                    "rebuild broken paru from AUR source · executable {} · {reason}",
                    path.display()
                ),
            ),
        }
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        match plan {
            ParuInstallPlan::Install { reason } => ctx.log().debug(format!(
                "paru install attempted · missing from target · {reason}"
            )),
            ParuInstallPlan::Rebuild { path, reason } => ctx.log().debug(format!(
                "paru rebuild attempted · executable {} · {reason}",
                path.display()
            )),
        }
        check_prerequisites(ctx)?;
        let guard =
            crate::infra::fs::TempGuard::create_unique_dir(self.build_root, "paru-build", "")
                .context("reserving paru build directory")?;
        clone_paru_from_aur(ctx, guard.path())?;
        if let Some(cache) = self.inventory_cache {
            cache.invalidate();
        }
        build_paru(ctx, guard.path())
            .with_context(|| format!("paru {} attempt failed", plan.action()))?;

        match check_paru_health(ctx.executor())? {
            ParuHealth::Healthy {
                path,
                package,
                version,
            } => ctx.log().action(
                plan.action(),
                "paru",
                false,
                &format!(
                    "{} paru · target validated · package {package} · executable {} · {version}",
                    plan.action(),
                    path.display()
                ),
            ),
            ParuHealth::Missing { reason } => anyhow::bail!(
                "paru {} completed but the target package was not found during validation: {reason}",
                plan.action()
            ),
            ParuHealth::Broken { path, reason } => anyhow::bail!(
                "paru {} completed but target executable {} failed validation: {reason}",
                plan.action(),
                path.display()
            ),
        }

        // Run log only: the status row already reports the completed action.
        ctx.log().trace(format!(
            "paru {} and validated successfully",
            plan.completed_action()
        ));
        Ok(TaskStats::changed_with_message(format!("{} paru", plan.completed_action())).finish())
    }
}

#[derive(Debug)]
struct PackageInstallOperation {
    packages: Vec<Package>,
    manager: PackageManager,
    inventory_cache: Option<PacmanInventoryCache>,
}

impl PackageInstallOperation {
    const fn new(packages: Vec<Package>, manager: PackageManager) -> Self {
        Self {
            packages,
            manager,
            inventory_cache: None,
        }
    }

    fn with_inventory_cache(mut self, inventory_cache: Option<PacmanInventoryCache>) -> Self {
        self.inventory_cache = inventory_cache;
        self
    }

    fn plan(&self, ctx: &Context) -> Result<PackageInstallPlan> {
        build_install_plan(
            ctx,
            &self.packages,
            self.manager,
            self.inventory_cache.as_ref(),
        )
    }
}

impl Operation for PackageInstallOperation {
    type Plan = PackageInstallPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let plan = self.plan(ctx)?;
        if plan.missing.is_empty() && plan.unavailable.is_empty() {
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(plan))
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        plan.report_unavailable(ctx);
        if self.manager == PackageManager::Pacman && !plan.missing.is_empty() {
            ctx.log().warn(
                "Installing missing packages with pacman -Syu will sync package databases and may upgrade other installed packages.",
            );
        }
        for resource in &plan.missing {
            let description = resource.description();
            ctx.log().action(
                "install",
                &description,
                true,
                &format!("install {description}"),
            );
        }
        Ok(plan.preview_stats().finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        plan.report_unavailable(ctx);
        if plan.missing.is_empty() {
            return Ok(plan.base_stats().finish());
        }
        ctx.log().debug(format!(
            "installing {} missing packages",
            plan.missing.len()
        ));
        let missing_refs: Vec<&PackageResource> = plan.missing.iter().collect();
        // Name packages before their batch or individual install starts;
        // slow installs and Windows UAC prompts otherwise hide the active work.
        let progress = |package: &str| {
            ctx.log()
                .action("install", package, false, &format!("install {package}"));
        };
        if let Some(cache) = &self.inventory_cache
            && matches!(self.manager, PackageManager::Pacman | PackageManager::Paru)
        {
            cache.invalidate();
        }
        let report = match install_missing_packages(
            self.manager,
            &missing_refs,
            ctx.executor(),
            &progress,
        ) {
            Ok(report) => report,
            Err(mut e) => {
                if let Some(report) = e.downcast_mut::<BatchReport>() {
                    report.merge_stats(&plan.base_stats());
                    return Err(e);
                }
                if e.downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
                {
                    return Err(e.context(BatchReport::new(
                        plan.base_stats(),
                        BatchCompletion::Interrupted,
                        u32::try_from(plan.missing.len()).unwrap_or(u32::MAX),
                        0,
                    )));
                }
                let reason = format!("{} install failed: {e:#}", self.manager);
                ctx.log().warn(&reason);
                let stats = TaskStats::from_counts(
                    0,
                    plan.base_stats().already_ok_count(),
                    0,
                    u32::try_from(plan.missing.len() + plan.unavailable.len()).unwrap_or(u32::MAX),
                );
                return Ok(stats.finish());
            }
        };

        for package in report.pending_reboot() {
            ctx.log().warn(format!(
                "{package} requires a reboot to finish installing; reboot before retrying dependent tasks"
            ));
        }
        for failure in report.failures() {
            ctx.log().warn(format!(
                "failed to install {} with {}: {}",
                failure.package, self.manager, failure.reason
            ));
        }

        let mut stats = plan.base_stats();
        stats.merge(&report.stats());

        if report.has_failures() {
            ctx.log().warn(format!(
                "{} package install(s) failed",
                report.failures().len()
            ));
        }

        Ok(stats.finish())
    }
}

#[cfg(test)]
mod tests;
