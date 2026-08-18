//! Tasks: install system packages.

use anyhow::{Context as _, Result};

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
use crate::infra::exec::ExecError;
use crate::infra::logging::OutputExt as _;

mod paru;
mod planning;

use paru::{
    ParuHealth, build_paru, check_paru_health, check_prerequisites, clone_paru_from_aur,
    prepare_build_directory,
};
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
        name: "System packages",
        selector: "packages",
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
            return Ok(TaskResult::NotApplicable(reason.to_string()));
        }

        let manager = match self {
            Self::Native => {
                ctx.trace_fmt(|| format!("{} non-AUR packages to process", selected.len()));
                match resolve_native_manager(ctx) {
                    Ok(manager) => manager,
                    Err(reason) => return Ok(TaskResult::unmet(reason)),
                }
            }
            Self::Aur => {
                let path = match check_paru_health(ctx.executor()) {
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
        name: "AUR packages",
        selector: "aur-packages",
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
        name: "Paru package manager",
        selector: "paru",
        deps: [InstallPackages],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().uses_pacman()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        // makepkg -si calls sudo internally to install the built package
        ctx.system().platform().uses_pacman()
            && !matches!(
                check_paru_health(ctx.executor()),
                ParuHealth::Healthy { .. }
            )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ParuInstallOperation)
    }
}

#[derive(Debug, Clone, Copy)]
struct ParuInstallOperation;

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

impl Operation for ParuInstallOperation {
    type Plan = ParuInstallPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        match check_paru_health(ctx.executor()) {
            ParuHealth::Missing { reason } => {
                ctx.log().debug(format!("paru status: missing · {reason}"));
                Ok(OperationState::needs_run(
                    "install missing paru from AUR source",
                    ParuInstallPlan::Install { reason },
                ))
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
                Ok(OperationState::needs_run(
                    format!("rebuild broken paru at {}", path.display()),
                    ParuInstallPlan::Rebuild { path, reason },
                ))
            }
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        match plan {
            ParuInstallPlan::Install { reason } => ctx
                .log()
                .dry_run(format!("install missing paru from AUR source · {reason}")),
            ParuInstallPlan::Rebuild { path, reason } => ctx.log().dry_run(format!(
                "rebuild broken paru from AUR source · executable {} · {reason}",
                path.display()
            )),
        }
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        match plan {
            ParuInstallPlan::Install { reason } => ctx.log().info(format!(
                "paru install attempted · missing from target · {reason}"
            )),
            ParuInstallPlan::Rebuild { path, reason } => ctx.log().info(format!(
                "paru rebuild attempted · executable {} · {reason}",
                path.display()
            )),
        }
        check_prerequisites(ctx)?;
        let guard = crate::infra::fs::TempGuard::dir(prepare_build_directory(ctx)?);
        clone_paru_from_aur(ctx, guard.path())?;
        build_paru(ctx, guard.path())
            .with_context(|| format!("paru {} attempt failed", plan.action()))?;

        match check_paru_health(ctx.executor()) {
            ParuHealth::Healthy {
                path,
                package,
                version,
            } => ctx.log().info(format!(
                "{} paru passed target validation · package {package} · executable {} · {version}",
                plan.completed_action(),
                path.display()
            )),
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
                .dry_run(format!("install {}", resource.description()));
        }
        Ok(plan.preview_stats().finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log().debug(format!(
            "installing {} missing packages",
            plan.missing.len()
        ));
        let missing_refs: Vec<&PackageResource> = plan.missing.iter().collect();
        // Installs run one at a time and can each be slow — or, on Windows,
        // raise their own UAC prompt — so name the package before starting it
        // rather than reporting only once the whole batch is done.
        let progress = |package: &str| ctx.log().info(format!("install {package}"));
        let report = match install_missing_packages(
            self.manager,
            &missing_refs,
            ctx.system().executor(),
            &progress,
        ) {
            Ok(report) => report,
            Err(e) => {
                if e.downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
                {
                    return Err(e);
                }
                let reason = format!("{} install failed: {e:#}", self.manager);
                ctx.log().warn(&reason);
                let stats = TaskStats::from_counts(
                    0,
                    plan.base_stats().already_ok_count(),
                    0,
                    u32::try_from(plan.missing.len()).unwrap_or(u32::MAX),
                );
                return Ok(stats.finish());
            }
        };

        for failure in report.failures() {
            ctx.log().warn(format!(
                "failed to install {} with {}: {}",
                failure.package, self.manager, failure.reason
            ));
        }

        let stats = TaskStats::from_counts(
            u32::try_from(report.applied_count()).unwrap_or(u32::MAX),
            plan.base_stats()
                .already_ok_count()
                .saturating_add(u32::try_from(report.already_correct_count()).unwrap_or(u32::MAX)),
            0,
            u32::try_from(report.failures().len()).unwrap_or(u32::MAX),
        );

        if report.has_failures() {
            let reason = format!("{} package install(s) failed", report.failures().len());
            ctx.log().warn(&reason);
            return Ok(stats.finish());
        }

        Ok(stats.finish())
    }
}

#[cfg(test)]
mod tests;
