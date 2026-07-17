//! Tasks: install system packages.

use anyhow::Result;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    PackageManager, PackageResource, install_missing_packages,
};
use crate::engine::Resource as _;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, process_operation, task_metadata,
};
use crate::infra::ConfigHandle;

mod paru;
mod planning;

use paru::{build_paru, check_prerequisites, clone_paru_from_aur, prepare_build_directory};
use planning::{
    PackageInstallPlan, build_install_plan, predict_sudo, resolve_native_manager, select_packages,
};

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Install system packages via pacman or winget.
#[derive(Debug)]
pub struct InstallPackages {
    config: ConfigHandle<Vec<Package>>,
}

impl InstallPackages {
    /// Create the task with a handle to the package configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<Package>>) -> Self {
        Self { config }
    }
}

impl Task for InstallPackages {
    task_metadata! {
        name: "Install packages",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        self.config.read().iter().any(|p| !p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.system().platform().uses_pacman() {
            return false;
        }
        predict_sudo(
            ctx,
            PackageManager::Pacman,
            "pacman",
            &select_packages(&self.config.read(), false),
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages = select_packages(&self.config.read(), false);
        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no packages to install".to_string()));
        }

        ctx.log()
            .debug(&format!("{} non-AUR packages to process", packages.len()));

        let manager = match resolve_native_manager(ctx) {
            Ok(m) => m,
            Err(reason) => return Ok(TaskResult::Skipped(reason)),
        };

        process_operation(ctx, &PackageInstallOperation::new(packages, manager))
    }
}

/// Install AUR packages via paru.
#[derive(Debug)]
pub struct InstallAurPackages {
    config: ConfigHandle<Vec<Package>>,
}

impl InstallAurPackages {
    /// Create the task with a handle to the package configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<Package>>) -> Self {
        Self { config }
    }
}

impl Task for InstallAurPackages {
    task_metadata! {
        name: "Install AUR packages",
        deps: [InstallParu],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().supports_aur() && self.config.read().iter().any(|p| p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.system().platform().supports_aur() {
            return false;
        }
        predict_sudo(
            ctx,
            PackageManager::Paru,
            "paru",
            &select_packages(&self.config.read(), true),
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages = select_packages(&self.config.read(), true);
        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no AUR packages".to_string()));
        }

        if !ctx.system().which("paru") {
            ctx.log()
                .debug("paru not found in PATH, skipping AUR packages");
            return Ok(TaskResult::Skipped("paru not installed".to_string()));
        }

        ctx.log()
            .debug(&format!("checking {} AUR packages", packages.len()));

        process_operation(
            ctx,
            &PackageInstallOperation::new(packages, PackageManager::Paru),
        )
    }
}

/// Install paru AUR helper.
#[derive(Debug)]
pub struct InstallParu;

impl Task for InstallParu {
    task_metadata! {
        name: "Install paru",
        deps: [InstallPackages],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().uses_pacman()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        // makepkg -si calls sudo internally to install the built package
        ctx.system().platform().uses_pacman() && !ctx.system().which("paru")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ParuInstallOperation)
    }
}

#[derive(Debug, Clone, Copy)]
struct ParuInstallOperation;

impl Operation for ParuInstallOperation {
    type Plan = ();

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        if ctx.system().which("paru") {
            ctx.log().debug("paru already in PATH");
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(
                "install paru from AUR (paru-bin)",
                (),
            ))
        }
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log().dry_run("install paru from AUR (paru-bin)");
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        check_prerequisites(ctx)?;
        let guard = crate::infra::fs::TempDir::new(prepare_build_directory(ctx)?);
        clone_paru_from_aur(ctx, guard.path())?;
        build_paru(ctx, guard.path())?;

        ctx.log().info("paru installed successfully");
        Ok(TaskResult::OkWithMessage("installed paru".to_string()))
    }
}

#[derive(Debug)]
struct PackageInstallOperation {
    packages: Vec<Package>,
    manager: PackageManager,
}

impl PackageInstallOperation {
    const fn new(packages: Vec<Package>, manager: PackageManager) -> Self {
        Self { packages, manager }
    }

    fn plan(&self, ctx: &Context) -> Result<PackageInstallPlan> {
        build_install_plan(ctx, &self.packages, self.manager)
    }
}

impl Operation for PackageInstallOperation {
    type Plan = PackageInstallPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let plan = self.plan(ctx)?;
        if plan.missing.is_empty() {
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(
                format!("install {} missing package(s)", plan.missing.len()),
                plan,
            ))
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        for resource in &plan.missing {
            ctx.log()
                .dry_run(&format!("would install: {}", resource.description()));
        }
        Ok(plan.preview_stats().finish(ctx))
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log().debug(&format!(
            "installing {} missing packages",
            plan.missing.len()
        ));
        let missing_refs: Vec<&PackageResource> = plan.missing.iter().collect();
        let report =
            match install_missing_packages(self.manager, &missing_refs, ctx.system().executor()) {
                Ok(report) => report,
                Err(e) => {
                    let reason = format!("{} install failed: {e:#}", self.manager);
                    ctx.log().warn(&reason);
                    let mut stats = plan.base_stats();
                    stats.failed = u32::try_from(plan.missing.len()).unwrap_or(u32::MAX);
                    stats.log_summary(ctx);
                    return Ok(TaskResult::Failed(reason));
                }
            };

        for failure in report.failures() {
            ctx.log().warn(&format!(
                "failed to install {} with {}: {}",
                failure.package, self.manager, failure.reason
            ));
        }

        let mut stats = plan.base_stats();
        stats.changed = u32::try_from(report.applied_count()).unwrap_or(u32::MAX);
        stats.failed = u32::try_from(report.failures().len()).unwrap_or(u32::MAX);

        for package in report.applied_packages() {
            ctx.log().info(&format!("installed: {package}"));
        }

        if report.has_failures() {
            let reason = format!("{} package install(s) failed", report.failures().len());
            ctx.log().warn(&reason);
            stats.log_summary(ctx);
            return Ok(TaskResult::Failed(reason));
        }

        Ok(stats.finish(ctx))
    }
}
