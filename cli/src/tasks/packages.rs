use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

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

        if ctx.platform.is_linux() {
            ctx.log.debug("using pacman package manager");
            install_pacman(ctx, &packages)
        } else {
            ctx.log.debug("using winget package manager");
            install_winget(ctx, &packages)
        }
    }
}

/// Install AUR packages via paru.
pub struct InstallAurPackages;

impl Task for InstallAurPackages {
    fn name(&self) -> &'static str {
        "Install AUR packages"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux()
            && ctx.platform.is_arch
            && ctx.config.packages.iter().any(|p| p.is_aur)
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

        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        ctx.log.debug(&format!(
            "checking {} AUR packages against installed list",
            names.len()
        ));
        let installed = pacman_installed_set();

        let mut stats = TaskStats::new();
        let mut to_install: Vec<&str> = Vec::new();
        for name in &names {
            if installed.contains(*name) {
                ctx.log.debug(&format!("ok: {name} (already installed)"));
                stats.already_ok += 1;
            } else {
                to_install.push(name);
            }
        }

        if ctx.dry_run {
            for name in &to_install {
                ctx.log.dry_run(&format!("would install (AUR): {name}"));
            }
            stats.changed = to_install.len() as u32;
            return Ok(stats.finish(ctx));
        }

        if to_install.is_empty() {
            return Ok(stats.finish(ctx));
        }

        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(&to_install);
        ctx.log.debug(&format!(
            "running paru to install {} AUR packages",
            to_install.len()
        ));
        exec::run("paru", &args)?;

        stats.changed = to_install.len() as u32;
        Ok(stats.finish(ctx))
    }
}

/// Install paru AUR helper.
pub struct InstallParu;

impl Task for InstallParu {
    fn name(&self) -> &'static str {
        "Install paru"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && ctx.platform.is_arch
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

/// Query pacman for all installed package names.
fn pacman_installed_set() -> std::collections::HashSet<String> {
    exec::run_unchecked("pacman", &["-Q"])
        .map(|r| {
            r.stdout
                .lines()
                .filter_map(|l| l.split_whitespace().next().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn install_pacman(
    ctx: &Context,
    packages: &[&crate::config::packages::Package],
) -> Result<TaskResult> {
    if !exec::which("pacman") {
        ctx.log.debug("pacman not found in PATH");
        return Ok(TaskResult::Skipped("pacman not found".to_string()));
    }

    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    ctx.log.debug("querying installed packages via pacman -Q");
    let installed = pacman_installed_set();

    let mut stats = TaskStats::new();
    let mut to_install: Vec<&str> = Vec::new();
    for name in &names {
        if installed.contains(*name) {
            ctx.log.debug(&format!("ok: {name} (already installed)"));
            stats.already_ok += 1;
        } else {
            to_install.push(name);
        }
    }

    if ctx.dry_run {
        for name in &to_install {
            ctx.log.dry_run(&format!("would install: {name}"));
        }
        stats.changed = to_install.len() as u32;
        return Ok(stats.finish(ctx));
    }

    if to_install.is_empty() {
        return Ok(stats.finish(ctx));
    }

    let mut args = vec!["-S", "--needed", "--noconfirm"];
    args.extend(&to_install);
    ctx.log.debug(&format!(
        "running sudo pacman to install {} packages",
        to_install.len()
    ));
    exec::run("sudo", &{
        let mut a = vec!["pacman"];
        a.extend(&args);
        a
    })?;

    stats.changed = to_install.len() as u32;
    Ok(stats.finish(ctx))
}

fn install_winget(
    ctx: &Context,
    packages: &[&crate::config::packages::Package],
) -> Result<TaskResult> {
    if !exec::which("winget") {
        ctx.log.debug("winget not found in PATH");
        return Ok(TaskResult::Skipped("winget not found".to_string()));
    }

    // Query installed packages once upfront
    ctx.log.debug("querying installed packages via winget list");
    let list = exec::run_unchecked("winget", &["list", "--accept-source-agreements"])
        .map(|r| r.stdout)
        .unwrap_or_default();

    let mut stats = TaskStats::new();
    let mut to_install: Vec<&str> = Vec::new();

    for pkg in packages {
        if list.contains(&pkg.name) {
            ctx.log.debug(&format!("{} is installed", pkg.name));
            stats.already_ok += 1;
        } else {
            to_install.push(&pkg.name);
        }
    }

    if ctx.dry_run {
        for name in &to_install {
            ctx.log.dry_run(&format!("would install: {name}"));
        }
        stats.changed = to_install.len() as u32;
        return Ok(stats.finish(ctx));
    }

    for name in &to_install {
        let result = exec::run_unchecked(
            "winget",
            &[
                "install",
                "--id",
                name,
                "--exact",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ],
        )?;

        if result.success {
            ctx.log.debug(&format!("{name} installed"));
            stats.changed += 1;
        } else {
            ctx.log.info(&format!("{name} failed to install"));
        }
    }

    Ok(stats.finish(ctx))
}
