//! Tasks: install system packages.

use anyhow::{Context as _, Result};
use std::collections::HashSet;

use crate::config::packages::Package;
use crate::resources::package::{PackageManager, PackageResource, get_installed_packages};
use crate::resources::{Resource as _, ResourceState};
use crate::tasks::{
    Context, Domain, ExecutionPolicy, PlatformCapability, Task, TaskPhase, TaskResult, TaskStats,
    task_metadata,
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

        process_packages(ctx, &packages, manager)
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

        process_packages(ctx, &packages, PackageManager::Paru)
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
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().uses_pacman()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        // makepkg -si calls sudo internally to install the built package
        ctx.system().platform().uses_pacman() && !ctx.system().which("paru")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.system().which("paru") {
            ctx.log.debug("paru already in PATH");
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log.dry_run("install paru from AUR (paru-bin)");
            return Ok(TaskResult::DryRun);
        }

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

/// Install missing packages using the package provider's preferred strategy.
///
/// Pacman and Paru install in one solver invocation; Winget installs
/// individually and reports per-package failures without aborting the whole set.
fn install_missing(
    ctx: &Context,
    packages: &[Package],
    installed: &HashSet<String>,
    manager: PackageManager,
) -> TaskResult {
    let system = ctx.system();
    let resources: Vec<PackageResource> = packages
        .iter()
        .map(|pkg| PackageResource::new(pkg.name.clone(), manager, system.executor_arc()))
        .collect();

    let mut stats = TaskStats::new();
    let mut missing = Vec::new();

    for r in &resources {
        if matches!(r.state_from_installed(installed), ResourceState::Correct) {
            ctx.debug_fmt(|| format!("ok: {}", r.description()));
            stats.already_ok = stats.already_ok.saturating_add(1);
        } else {
            missing.push(r);
        }
    }

    if missing.is_empty() {
        return stats.finish(ctx);
    }

    if ctx.dry_run {
        for r in &missing {
            ctx.log
                .dry_run(&format!("would install: {}", r.description()));
        }
        stats.changed = u32::try_from(missing.len()).unwrap_or(u32::MAX);
        return stats.finish(ctx);
    }

    ctx.log
        .debug(&format!("installing {} missing packages", missing.len()));
    let report = match manager
        .provider()
        .install_missing(&missing, system.executor())
    {
        Ok(report) => report,
        Err(e) => {
            let reason = format!("{manager} install failed: {e:#}");
            ctx.log.warn(&reason);
            stats.failed = u32::try_from(missing.len()).unwrap_or(u32::MAX);
            drop(stats.finish(ctx));
            return TaskResult::Failed(reason);
        }
    };

    for failure in report.failures() {
        ctx.log.warn(&format!(
            "failed to install {} with {manager}: {}",
            failure.package, failure.reason
        ));
    }

    let applied_count = report.applied_count();
    stats.changed = u32::try_from(applied_count).unwrap_or(u32::MAX);
    stats.failed = u32::try_from(report.failures().len()).unwrap_or(u32::MAX);

    for package in report.applied_packages() {
        ctx.log.info(&format!("installed: {package}"));
    }

    if report.has_failures() {
        let reason = format!("{} package install(s) failed", report.failures().len());
        ctx.log.warn(&reason);
        drop(stats.finish(ctx));
        return TaskResult::Failed(reason);
    }

    stats.finish(ctx)
}

/// Process a list of packages by querying installed state once and dispatching
/// to the package provider's preferred install strategy.
fn process_packages(
    ctx: &Context,
    packages: &[Package],
    manager: PackageManager,
) -> Result<TaskResult> {
    ctx.debug_fmt(|| {
        format!(
            "batch-checking {} packages with a single query",
            packages.len()
        )
    });
    let installed = get_installed_packages(manager, ctx.system().executor())?;
    Ok(install_missing(ctx, packages, &installed, manager))
}
