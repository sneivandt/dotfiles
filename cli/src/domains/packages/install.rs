//! Tasks: install system packages.

use anyhow::{Context as _, Result};
use std::path::Path;

use crate::domains::packages::config::packages::Package;
use crate::domains::packages::resources::package::{
    PackageManager, PackageResource, install_missing_packages,
};
use crate::engine::Resource as _;
use crate::engine::{
    BatchCompletion, BatchReport, Context, Operation, OperationState, Task, TaskResult, TaskStats,
    process_operation, task_metadata,
};
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

crate::engine::config_task! {
    /// Install system packages via pacman or winget.
    pub struct InstallPackages(Vec<Package>);
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
        PackageTaskKind::Native.needs_elevation(ctx, self.config.get())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Native.run(ctx, self.config.get())
    }
}

crate::engine::config_task! {
    /// Install AUR packages via paru.
    pub struct InstallAurPackages(Vec<Package>);
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

    fn needs_elevation(self, ctx: &Context, packages: &[Package]) -> bool {
        let platform = ctx.platform();
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

        process_operation(ctx, &PackageInstallOperation::new(selected, manager))
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
                super::local_builds::ConfigureParu,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        PackageTaskKind::Aur.should_run(ctx, self.config.get())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        PackageTaskKind::Aur.needs_elevation(ctx, self.config.get())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        PackageTaskKind::Aur.run(ctx, self.config.get())
    }
}

/// Install paru AUR helper.
#[derive(Debug)]
pub struct InstallParu;

fn run_paru_install(ctx: &Context, build_dir: &Path) -> Result<TaskResult> {
    process_operation(ctx, &ParuInstallOperation { build_dir })
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
        run_paru_install(ctx, &std::env::temp_dir().join("paru-build"))
    }
}

#[derive(Debug, Clone, Copy)]
struct ParuInstallOperation<'a> {
    build_dir: &'a Path,
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
            ParuInstallPlan::Rebuild { path, reason } => ctx.log().dry_run(format!(
                "rebuild broken paru from AUR source · executable {} · {reason}",
                path.display()
            )),
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
        prepare_build_directory(ctx, self.build_dir)?;
        let guard = crate::infra::fs::TempGuard::dir(self.build_dir.to_path_buf());
        clone_paru_from_aur(ctx, guard.path())?;
        build_paru(ctx, guard.path())
            .with_context(|| format!("paru {} attempt failed", plan.action()))?;

        match check_paru_health(ctx.executor())? {
            ParuHealth::Healthy {
                path,
                package,
                version,
            } => ctx.log().info(format!(
                "{} paru · target validated · package {package} · executable {} · {version}",
                plan.action(),
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
        if plan.missing.is_empty() && plan.unavailable.is_empty() {
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(plan))
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        plan.report_unavailable(ctx);
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
