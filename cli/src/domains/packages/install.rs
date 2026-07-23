//! Tasks: install system packages.

use anyhow::Result;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    PackageManager, PackageResource, install_missing_packages,
};
use crate::engine::Resource as _;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, process_operation,
    task_metadata,
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

    fn should_run(&self, ctx: &Context) -> bool {
        PackageTaskKind::Native.should_run(ctx, &self.config.read())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        PackageTaskKind::Native.needs_elevation(ctx, &self.config.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Native.run(ctx, &self.config.read())
    }
}

/// Install AUR packages via paru.
#[derive(Debug)]
pub struct InstallAurPackages {
    config: ConfigHandle<Vec<Package>>,
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
            Self::Aur => ctx.system().platform().supports_aur(),
        };
        platform_is_supported
            && packages
                .iter()
                .any(|package| package.is_aur == self.is_aur())
    }

    fn needs_elevation(self, ctx: &Context, packages: &[Package]) -> bool {
        let platform = ctx.system().platform();
        let (supported, manager, executable) = match self {
            Self::Native => (platform.uses_pacman(), PackageManager::Pacman, "pacman"),
            Self::Aur => (platform.supports_aur(), PackageManager::Paru, "paru"),
        };
        supported && predict_sudo(ctx, manager, executable, &self.select(packages))
    }

    fn run(self, ctx: &Context, packages: &[Package]) -> Result<TaskResult> {
        let selected = self.select(packages);
        if selected.is_empty() {
            let reason = match self {
                Self::Native => "no packages to install",
                Self::Aur => "no AUR packages",
            };
            return Ok(TaskResult::Skipped(reason.to_string()));
        }

        let manager = match self {
            Self::Native => {
                ctx.debug_fmt(|| format!("{} non-AUR packages to process", selected.len()));
                match resolve_native_manager(ctx) {
                    Ok(manager) => manager,
                    Err(reason) => return Ok(TaskResult::Skipped(reason)),
                }
            }
            Self::Aur => {
                if !ctx.system().which("paru") {
                    ctx.log()
                        .debug("paru not found in PATH, skipping AUR packages");
                    return Ok(TaskResult::Skipped("paru not installed".to_string()));
                }
                ctx.debug_fmt(|| format!("checking {} AUR packages", selected.len()));
                PackageManager::Paru
            }
        };

        process_operation(ctx, &PackageInstallOperation::new(selected, manager))
    }
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
        PackageTaskKind::Aur.should_run(ctx, &self.config.read())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        PackageTaskKind::Aur.needs_elevation(ctx, &self.config.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Aur.run(ctx, &self.config.read())
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
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        check_prerequisites(ctx)?;
        let guard = crate::infra::fs::TempDir::new(prepare_build_directory(ctx)?);
        clone_paru_from_aur(ctx, guard.path())?;
        build_paru(ctx, guard.path())?;

        ctx.log().info("paru installed successfully");
        Ok(TaskStats::changed_with_message("installed paru").finish())
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
                .dry_run(&format!("install {}", resource.description()));
        }
        Ok(plan.preview_stats().finish())
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
                    return Ok(stats.finish());
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
            ctx.log().info(&format!("install {package}"));
        }

        if report.has_failures() {
            let reason = format!("{} package install(s) failed", report.failures().len());
            ctx.log().warn(&reason);
            return Ok(stats.finish());
        }

        Ok(stats.finish())
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
