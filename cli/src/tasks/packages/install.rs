//! Tasks: install system packages.

use anyhow::{Context as _, Result};

use crate::config::packages::Package;
use crate::resources::package::{
    PackageManager, PackageResource, get_installed_packages, install_missing_packages,
};
use crate::resources::{Resource as _, ResourceState};
use crate::tasks::{
    Context, Domain, ExecutionPolicy, Operation, OperationState, PlatformCapability, Task,
    TaskPhase, TaskResult, TaskStats, process_operation, task_metadata,
};

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

// ---------------------------------------------------------------------------
// Shared helpers — filter, manager detection, and sudo prediction
// ---------------------------------------------------------------------------

/// Collect packages matching the AUR/native filter from the loaded config.
fn select_packages(ctx: &Context, is_aur: bool) -> Vec<Package> {
    ctx.config_read()
        .packages
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
fn resolve_native_manager(ctx: &Context) -> Result<PackageManager, String> {
    let system = ctx.system();
    if system.platform().is_linux() {
        ctx.log.debug("using pacman package manager");
        if !system.which("pacman") {
            return Err("pacman not found".to_string());
        }
        Ok(PackageManager::Pacman)
    } else {
        ctx.log.debug("using winget package manager");
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
fn predict_sudo(ctx: &Context, manager: PackageManager, tool: &str, packages: &[Package]) -> bool {
    let system = ctx.system();
    if !system.which(tool) || packages.is_empty() {
        return false;
    }
    let Ok(installed) = get_installed_packages(manager, system.executor()) else {
        return false;
    };
    packages.iter().any(|p| !installed.contains(&p.name))
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Install system packages via pacman or winget.
#[derive(Debug)]
pub struct InstallPackages;

impl Task for InstallPackages {
    task_metadata! {
        name: "Install packages",
        phase: TaskPhase::Provision,
        domain: Domain::Packages,
        policy: [ExecutionPolicy::RequiresElevation],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().packages.iter().any(|p| !p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.system().platform().uses_pacman() {
            return false;
        }
        predict_sudo(
            ctx,
            PackageManager::Pacman,
            "pacman",
            &select_packages(ctx, false),
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages = select_packages(ctx, false);
        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no packages to install".to_string()));
        }

        ctx.log
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
pub struct InstallAurPackages;

impl Task for InstallAurPackages {
    task_metadata! {
        name: "Install AUR packages",
        phase: TaskPhase::Provision,
        domain: Domain::Packages,
        policy: [PlatformCapability::Aur.policy(), ExecutionPolicy::RequiresElevation],
        deps: [InstallParu],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().supports_aur()
            && ctx.config_read().packages.iter().any(|p| p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.system().platform().supports_aur() {
            return false;
        }
        predict_sudo(
            ctx,
            PackageManager::Paru,
            "paru",
            &select_packages(ctx, true),
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages = select_packages(ctx, true);
        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no AUR packages".to_string()));
        }

        if !ctx.system().which("paru") {
            ctx.log
                .debug("paru not found in PATH, skipping AUR packages");
            return Ok(TaskResult::Skipped("paru not installed".to_string()));
        }

        ctx.log
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
        phase: TaskPhase::Provision,
        domain: Domain::Packages,
        policy: [
            PlatformCapability::Pacman.policy(),
            ExecutionPolicy::RequiresElevation,
        ],
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
            ctx.log.debug("paru already in PATH");
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(
                "install paru from AUR (paru-bin)",
                (),
            ))
        }
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log.dry_run("install paru from AUR (paru-bin)");
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        check_prerequisites(ctx)?;
        let guard = crate::fs::TempDir::new(prepare_build_directory(ctx)?);
        clone_paru_from_aur(ctx, guard.path())?;
        build_paru(ctx, guard.path())?;

        ctx.log.info("paru installed successfully");
        Ok(TaskResult::OkWithMessage("installed paru".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Paru build helpers
// ---------------------------------------------------------------------------

/// Check that required tools are available for building paru.
fn check_prerequisites(ctx: &Context) -> Result<()> {
    for dep in ["git", "makepkg", "sudo"] {
        if !ctx.executor.which(dep) {
            anyhow::bail!("missing prerequisite: {dep}");
        }
        ctx.debug_fmt(|| format!("prerequisite ok: {dep}"));
    }
    Ok(())
}

/// Prepare a clean build directory for paru.
fn prepare_build_directory(ctx: &Context) -> Result<std::path::PathBuf> {
    let tmp = std::env::temp_dir().join("paru-build");
    if tmp.exists() {
        ctx.log.debug("removing previous paru build directory");
        std::fs::remove_dir_all(&tmp).context("removing previous paru build directory")?;
    }
    Ok(tmp)
}

/// Clone the paru-bin AUR package.
fn clone_paru_from_aur(ctx: &Context, tmp: &std::path::Path) -> Result<()> {
    ctx.log.debug("cloning paru-bin from AUR");
    ctx.executor
        .run(
            "git",
            &[
                "clone",
                "https://aur.archlinux.org/paru-bin.git",
                &tmp.to_string_lossy(),
            ],
        )
        .context("cloning paru-bin from AUR")?;
    Ok(())
}

/// Build paru using makepkg with parallel compilation.
fn build_paru(ctx: &Context, tmp: &std::path::Path) -> Result<()> {
    let nproc = std::thread::available_parallelism()
        .map_or_else(|_| DEFAULT_NPROC.to_string(), |n| n.get().to_string());

    let makeflags = format!("-j{nproc}");
    ctx.log
        .debug(&format!("building with MAKEFLAGS={makeflags}"));
    ctx.executor
        .run_in_with_env(
            tmp,
            "makepkg",
            &["-si", "--noconfirm"],
            &[("MAKEFLAGS", &makeflags)],
        )
        .context("building paru with makepkg")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Package installation
// ---------------------------------------------------------------------------

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
        ctx.debug_fmt(|| {
            format!(
                "batch-checking {} packages with a single query",
                self.packages.len()
            )
        });

        let system = ctx.system();
        let installed = get_installed_packages(self.manager, system.executor())?;
        let resources: Vec<PackageResource> = self
            .packages
            .iter()
            .map(|pkg| PackageResource::new(pkg.name.clone(), self.manager, system.executor_arc()))
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
            ctx.log
                .dry_run(&format!("would install: {}", resource.description()));
        }
        Ok(plan.preview_stats().finish(ctx))
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log.debug(&format!(
            "installing {} missing packages",
            plan.missing.len()
        ));
        let missing_refs: Vec<&PackageResource> = plan.missing.iter().collect();
        let report =
            match install_missing_packages(self.manager, &missing_refs, ctx.system().executor()) {
                Ok(report) => report,
                Err(e) => {
                    let reason = format!("{} install failed: {e:#}", self.manager);
                    ctx.log.warn(&reason);
                    let mut stats = plan.base_stats();
                    stats.failed = u32::try_from(plan.missing.len()).unwrap_or(u32::MAX);
                    stats.log_summary(ctx);
                    return Ok(TaskResult::Failed(reason));
                }
            };

        for failure in report.failures() {
            ctx.log.warn(&format!(
                "failed to install {} with {}: {}",
                failure.package, self.manager, failure.reason
            ));
        }

        let mut stats = plan.base_stats();
        stats.changed = u32::try_from(report.applied_count()).unwrap_or(u32::MAX);
        stats.failed = u32::try_from(report.failures().len()).unwrap_or(u32::MAX);

        for package in report.applied_packages() {
            ctx.log.info(&format!("installed: {package}"));
        }

        if report.has_failures() {
            let reason = format!("{} package install(s) failed", report.failures().len());
            ctx.log.warn(&reason);
            stats.log_summary(ctx);
            return Ok(TaskResult::Failed(reason));
        }

        Ok(stats.finish(ctx))
    }
}

#[derive(Debug, Clone)]
struct PackageInstallPlan {
    missing: Vec<PackageResource>,
    already_ok: usize,
}

impl PackageInstallPlan {
    fn base_stats(&self) -> TaskStats {
        let mut stats = TaskStats::new();
        stats.already_ok = u32::try_from(self.already_ok).unwrap_or(u32::MAX);
        stats
    }

    fn preview_stats(&self) -> TaskStats {
        let mut stats = self.base_stats();
        stats.changed = u32::try_from(self.missing.len()).unwrap_or(u32::MAX);
        stats
    }
}
