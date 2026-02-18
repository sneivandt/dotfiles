use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;
use crate::resources::package::{PackageManager, PackageResource, get_installed_packages};
use crate::resources::{Resource, ResourceChange, ResourceState};

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
    let mut stats = TaskStats::new();

    // Single command to get all installed packages
    ctx.log.debug(&format!(
        "batch-checking {} packages with a single query",
        packages.len()
    ));
    let installed = get_installed_packages(manager)?;

    for pkg in packages {
        let resource = PackageResource::new(pkg.name.clone(), manager);
        let resource_state = resource.state_from_installed(&installed);

        match resource_state {
            ResourceState::Correct => {
                ctx.log.debug(&format!(
                    "ok: {} (already installed)",
                    resource.description()
                ));
                stats.already_ok += 1;
            }
            ResourceState::Missing | ResourceState::Incorrect { .. } => {
                if ctx.dry_run {
                    ctx.log
                        .dry_run(&format!("would install: {}", resource.description()));
                    stats.changed += 1;
                    continue;
                }

                match resource.apply() {
                    Ok(ResourceChange::Applied) => {
                        ctx.log
                            .debug(&format!("installed: {}", resource.description()));
                        stats.changed += 1;
                    }
                    Ok(ResourceChange::Skipped { reason }) => {
                        ctx.log
                            .warn(&format!("skipped {}: {reason}", resource.description()));
                        stats.skipped += 1;
                    }
                    Ok(ResourceChange::AlreadyCorrect) => {
                        stats.already_ok += 1;
                    }
                    Err(e) => {
                        ctx.log.warn(&format!(
                            "failed to install {}: {e}",
                            resource.description()
                        ));
                        stats.skipped += 1;
                    }
                }
            }
            ResourceState::Invalid { reason } => {
                ctx.log
                    .debug(&format!("skipping {}: {reason}", resource.description()));
                stats.skipped += 1;
            }
        }
    }

    Ok(stats.finish(ctx))
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

        // Check prerequisites
        for dep in &["git", "makepkg", "sudo"] {
            if !exec::which(dep) {
                anyhow::bail!("missing prerequisite: {dep}");
            }
            ctx.log.debug(&format!("prerequisite ok: {dep}"));
        }

        let tmp = std::env::temp_dir().join("paru-build");
        if tmp.exists() {
            ctx.log.debug("removing previous paru build directory");
            std::fs::remove_dir_all(&tmp)?;
        }

        ctx.log.debug("cloning paru-bin from AUR");
        exec::run(
            "git",
            &[
                "clone",
                "https://aur.archlinux.org/paru-bin.git",
                &tmp.to_string_lossy(),
            ],
        )?;

        // Build with parallel compilation
        let nproc = exec::run("nproc", &[]).map_or_else(
            |_| DEFAULT_NPROC.to_string(),
            |r| r.stdout.trim().to_string(),
        );

        let makeflags = format!("-j{nproc}");
        ctx.log
            .debug(&format!("building with MAKEFLAGS={makeflags}"));
        exec::run_in_with_env(
            &tmp,
            "makepkg",
            &["-si", "--noconfirm"],
            &[("MAKEFLAGS", &makeflags)],
        )?;

        // Cleanup (ignore errors - best effort)
        std::fs::remove_dir_all(&tmp).ok();

        ctx.log.info("paru installed successfully");
        Ok(TaskResult::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
