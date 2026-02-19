use anyhow::{Context as _, Result};

use super::{Context, ProcessOpts, Task, TaskResult, process_resource_states};
use crate::exec;
use crate::resources::package::{PackageManager, PackageResource, get_installed_packages};

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Process a list of packages using batch-checked installed state.
///
/// Queries all installed packages **once**, then iterates each package to
/// determine whether it needs to be installed. This is dramatically faster
/// than spawning a per-package query.
fn process_packages(
    ctx: &Context,
    packages: &[&crate::config::packages::Package],
    manager: PackageManager,
) -> Result<TaskResult> {
    ctx.log.debug(&format!(
        "batch-checking {} packages with a single query",
        packages.len()
    ));
    let installed = get_installed_packages(manager)?;

    let resource_states = packages.iter().map(|pkg| {
        let resource = PackageResource::new(pkg.name.clone(), manager);
        let state = resource.state_from_installed(&installed);
        (resource, state)
    });

    process_resource_states(
        ctx,
        resource_states,
        &ProcessOpts {
            verb: "install",
            fix_incorrect: true,
            fix_missing: true,
            bail_on_error: false,
        },
    )
}

/// Install system packages via pacman or winget.
pub struct InstallPackages;

impl Task for InstallPackages {
    fn name(&self) -> &'static str {
        "Install packages"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config.packages.iter().any(|p| !p.is_aur)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages: Vec<_> = ctx.config.packages.iter().filter(|p| !p.is_aur).collect();

        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no packages to install".to_string()));
        }

        ctx.log
            .debug(&format!("{} non-AUR packages to process", packages.len()));

        let manager = if ctx.platform.is_linux() {
            ctx.log.debug("using pacman package manager");
            if !exec::which("pacman") {
                return Ok(TaskResult::Skipped("pacman not found".to_string()));
            }
            PackageManager::Pacman
        } else {
            ctx.log.debug("using winget package manager");
            if !exec::which("winget") {
                return Ok(TaskResult::Skipped("winget not found".to_string()));
            }
            PackageManager::Winget
        };

        process_packages(ctx, &packages, manager)
    }
}

/// Install AUR packages via paru.
pub struct InstallAurPackages;

impl Task for InstallAurPackages {
    fn name(&self) -> &'static str {
        "Install AUR packages"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_aur() && ctx.config.packages.iter().any(|p| p.is_aur)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages: Vec<_> = ctx.config.packages.iter().filter(|p| p.is_aur).collect();

        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no AUR packages".to_string()));
        }

        if !exec::which("paru") {
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
pub struct InstallParu;

impl Task for InstallParu {
    fn name(&self) -> &'static str {
        "Install paru"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.uses_pacman()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if exec::which("paru") {
            ctx.log.debug("paru already in PATH");
            ctx.log.info("paru already installed");
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log.dry_run("install paru from AUR (paru-bin)");
            return Ok(TaskResult::DryRun);
        }

        check_prerequisites(ctx)?;
        let tmp = prepare_build_directory(ctx)?;
        clone_paru_from_aur(ctx, &tmp)?;
        build_paru(ctx, &tmp)?;
        cleanup_build_directory(&tmp);

        ctx.log.info("paru installed successfully");
        Ok(TaskResult::Ok)
    }
}

/// Check that required tools are available for building paru.
fn check_prerequisites(ctx: &Context) -> Result<()> {
    for dep in &["git", "makepkg", "sudo"] {
        if !exec::which(dep) {
            anyhow::bail!("missing prerequisite: {dep}");
        }
        ctx.log.debug(&format!("prerequisite ok: {dep}"));
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
    exec::run(
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
    let nproc = exec::run("nproc", &[]).map_or_else(
        |_| DEFAULT_NPROC.to_string(),
        |r| r.stdout.trim().to_string(),
    );

    let makeflags = format!("-j{nproc}");
    ctx.log
        .debug(&format!("building with MAKEFLAGS={makeflags}"));
    exec::run_in_with_env(
        tmp,
        "makepkg",
        &["-si", "--noconfirm"],
        &[("MAKEFLAGS", &makeflags)],
    )
    .context("building paru with makepkg")?;
    Ok(())
}

/// Remove the build directory (best effort, ignores errors).
fn cleanup_build_directory(tmp: &std::path::Path) {
    std::fs::remove_dir_all(tmp).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::resources::Resource;
    use crate::resources::package::PackageResource;

    #[test]
    fn package_resource_description() {
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman);
        assert_eq!(resource.description(), "git (pacman)");

        let resource = PackageResource::new("paru-bin".to_string(), PackageManager::Paru);
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource = PackageResource::new("Git.Git".to_string(), PackageManager::Winget);
        assert_eq!(resource.description(), "Git.Git (winget)");
    }
}
