//! Tasks: install system packages.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::config::packages::Package;
use crate::platform::Platform;
use crate::resources::package::{
    PackageManager, PackageResource, batch_install_packages, get_installed_packages,
};
use crate::resources::{BorrowedStateProvider, Resource as _};
use crate::tasks::{
    Context, Domain, ExecutionPolicy, ProcessOpts, Task, TaskPhase, TaskResult, TaskStats,
    process_resources_with_provider, task_deps,
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
    if ctx.platform.is_linux() {
        ctx.log.debug("using pacman package manager");
        if !ctx.executor.which("pacman") {
            return Err("pacman not found".to_string());
        }
        Ok(PackageManager::Pacman)
    } else {
        ctx.log.debug("using winget package manager");
        if !ctx.executor.which("winget") {
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
    if !ctx.executor.which(tool) || packages.is_empty() {
        return false;
    }
    let Ok(installed) = get_installed_packages(manager, &*ctx.executor) else {
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
    fn name(&self) -> &'static str {
        "Install packages"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn domain(&self) -> Domain {
        Domain::Packages
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::RequiresElevation];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().packages.iter().any(|p| !p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.platform.uses_pacman() {
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
    fn name(&self) -> &'static str {
        "Install AUR packages"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn domain(&self) -> Domain {
        Domain::Packages
    }

    task_deps![InstallParu];

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[
            ExecutionPolicy::PlatformSupported("AUR", Platform::supports_aur),
            ExecutionPolicy::RequiresElevation,
        ];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_aur() && ctx.config_read().packages.iter().any(|p| p.is_aur)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        if !ctx.platform.supports_aur() {
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

        if !ctx.executor.which("paru") {
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
    fn name(&self) -> &'static str {
        "Install paru"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn domain(&self) -> Domain {
        Domain::Packages
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[
            ExecutionPolicy::PlatformSupported("pacman", Platform::uses_pacman),
            ExecutionPolicy::RequiresElevation,
        ];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.uses_pacman()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        // makepkg -si calls sudo internally to install the built package
        ctx.platform.uses_pacman() && !ctx.executor.which("paru")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.executor.which("paru") {
            ctx.log.debug("paru already in PATH");
            if ctx.dry_run {
                return Ok(TaskResult::DryRun);
            }
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
        Ok(TaskResult::Ok)
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

/// Install missing packages in a single package-manager invocation.
///
/// Faster than per-package installs and lets the solver resolve cross-package
/// dependencies across the full set.  Used for managers whose
/// [`PackageProvider::supports_batch`](crate::resources::package::PackageProvider::supports_batch)
/// returns `true` (Pacman, Paru).
fn batch_install(
    ctx: &Context,
    packages: &[Package],
    installed: &HashSet<String>,
    manager: PackageManager,
) -> TaskResult {
    let resources: Vec<PackageResource> = packages
        .iter()
        .map(|pkg| PackageResource::new(pkg.name.clone(), manager, Arc::clone(&ctx.executor)))
        .collect();

    let mut stats = TaskStats::new();
    let mut missing = Vec::new();

    for r in &resources {
        if installed.contains(&r.name) {
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
        .debug(&format!("batch-installing {} packages", missing.len()));
    if let Err(e) = batch_install_packages(&missing) {
        let reason = format!("batch install failed: {e:#}");
        ctx.log.warn(&reason);
        stats.skipped = u32::try_from(missing.len()).unwrap_or(u32::MAX);
        drop(stats.finish(ctx));
        return TaskResult::Failed(reason);
    }
    stats.changed = u32::try_from(missing.len()).unwrap_or(u32::MAX);

    stats.finish(ctx)
}

/// Install packages one at a time so that one failure does not prevent the
/// remainder from being attempted.  Used for managers whose
/// [`PackageProvider::supports_batch`](crate::resources::package::PackageProvider::supports_batch)
/// returns `false` (Winget).
fn individual_install(
    ctx: &Context,
    packages: &[Package],
    installed: &HashSet<String>,
    manager: PackageManager,
) -> Result<TaskResult> {
    let resources = packages
        .iter()
        .map(|pkg| PackageResource::new(pkg.name.clone(), manager, Arc::clone(&ctx.executor)));
    let provider = BorrowedStateProvider::new(
        installed,
        |resource: &PackageResource, installed: &HashSet<String>| {
            Ok(resource.state_from_installed(installed))
        },
    );
    process_resources_with_provider(ctx, resources, &provider, &ProcessOpts::lenient("install"))
}

/// Process a list of packages by querying installed state once and dispatching
/// to either [`batch_install`] or [`individual_install`] based on whether the
/// underlying provider supports batch installation.
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
    let installed = get_installed_packages(manager, &*ctx.executor)?;

    if manager.provider().supports_batch() {
        Ok(batch_install(ctx, packages, &installed, manager))
    } else {
        individual_install(ctx, packages, &installed, manager)
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
