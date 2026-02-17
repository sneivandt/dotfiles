use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Install system packages via pacman or winget.
pub struct InstallPackages;

impl Task for InstallPackages {
    fn name(&self) -> &str {
        "Install packages"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        let non_aur: Vec<_> = ctx.config.packages.iter().filter(|p| !p.is_aur).collect();
        !non_aur.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages: Vec<_> = ctx.config.packages.iter().filter(|p| !p.is_aur).collect();

        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no packages to install".to_string()));
        }

        if ctx.platform.is_linux() {
            install_pacman(ctx, &packages)
        } else {
            install_winget(ctx, &packages)
        }
    }
}

/// Install AUR packages via paru.
pub struct InstallAurPackages;

impl Task for InstallAurPackages {
    fn name(&self) -> &str {
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
            return Ok(TaskResult::Skipped("paru not installed".to_string()));
        }

        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();

        let installed_list = exec::run_unchecked("pacman", &["-Q"])
            .map(|r| r.stdout)
            .unwrap_or_default();

        let mut already = 0u32;
        let mut to_install: Vec<&str> = Vec::new();
        for name in &names {
            if installed_list
                .lines()
                .any(|l| l.starts_with(&format!("{name} ")))
            {
                ctx.log.debug(&format!("ok: {name} (already installed)"));
                already += 1;
            } else {
                to_install.push(name);
            }
        }

        if ctx.dry_run {
            for name in &to_install {
                ctx.log.dry_run(&format!("would install (AUR): {name}"));
            }
            ctx.log.info(&format!(
                "{} would change, {already} already ok",
                to_install.len()
            ));
            return Ok(TaskResult::DryRun);
        }

        if to_install.is_empty() {
            ctx.log.info(&format!("0 changed, {already} already ok"));
            return Ok(TaskResult::Ok);
        }

        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(&to_install);
        exec::run("paru", &args)?;

        ctx.log.info(&format!(
            "{} changed, {already} already ok",
            to_install.len()
        ));
        Ok(TaskResult::Ok)
    }
}

/// Install paru AUR helper.
pub struct InstallParu;

impl Task for InstallParu {
    fn name(&self) -> &str {
        "Install paru"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && ctx.platform.is_arch
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if exec::which("paru") {
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
        }

        let tmp = std::env::temp_dir().join("paru-build");
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }

        exec::run(
            "git",
            &[
                "clone",
                "https://aur.archlinux.org/paru-bin.git",
                &tmp.to_string_lossy(),
            ],
        )?;

        // Build with parallel compilation
        let nproc = exec::run("nproc", &[])
            .map(|r| r.stdout.trim().to_string())
            .unwrap_or_else(|_| "4".to_string());

        let makeflags = format!("-j{nproc}");
        exec::run_in_with_env(
            &tmp,
            "makepkg",
            &["-si", "--noconfirm"],
            &[("MAKEFLAGS", &makeflags)],
        )?;

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);

        ctx.log.info("paru installed successfully");
        Ok(TaskResult::Ok)
    }
}

fn install_pacman(
    ctx: &Context,
    packages: &[&crate::config::packages::Package],
) -> Result<TaskResult> {
    if !exec::which("pacman") {
        return Ok(TaskResult::Skipped("pacman not found".to_string()));
    }

    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();

    let installed_list = exec::run_unchecked("pacman", &["-Q"])
        .map(|r| r.stdout)
        .unwrap_or_default();

    let mut already = 0u32;
    let mut to_install: Vec<&str> = Vec::new();
    for name in &names {
        if installed_list
            .lines()
            .any(|l| l.starts_with(&format!("{name} ")))
        {
            ctx.log.debug(&format!("ok: {name} (already installed)"));
            already += 1;
        } else {
            to_install.push(name);
        }
    }

    if ctx.dry_run {
        for name in &to_install {
            ctx.log.dry_run(&format!("would install: {name}"));
        }
        ctx.log.info(&format!(
            "{} would change, {already} already ok",
            to_install.len()
        ));
        return Ok(TaskResult::DryRun);
    }

    if to_install.is_empty() {
        ctx.log.info(&format!("0 changed, {already} already ok"));
        return Ok(TaskResult::Ok);
    }

    let mut args = vec!["-S", "--needed", "--noconfirm"];
    args.extend(&to_install);
    exec::run("sudo", &{
        let mut a = vec!["pacman"];
        a.extend(&args);
        a
    })?;

    ctx.log.info(&format!(
        "{} changed, {already} already ok",
        to_install.len()
    ));
    Ok(TaskResult::Ok)
}

fn install_winget(
    ctx: &Context,
    packages: &[&crate::config::packages::Package],
) -> Result<TaskResult> {
    if !exec::which("winget") {
        return Ok(TaskResult::Skipped("winget not found".to_string()));
    }

    // Query installed packages once upfront
    let list = exec::run_unchecked("winget", &["list", "--accept-source-agreements"])
        .map(|r| r.stdout)
        .unwrap_or_default();

    let mut to_install: Vec<&str> = Vec::new();
    let mut already = 0u32;

    for pkg in packages {
        if list.contains(&pkg.name) {
            ctx.log.debug(&format!("{} is installed", pkg.name));
            already += 1;
        } else {
            to_install.push(&pkg.name);
        }
    }

    if ctx.dry_run {
        for name in &to_install {
            ctx.log.dry_run(&format!("would install: {name}"));
        }
        ctx.log.info(&format!(
            "{} would change, {already} already ok",
            to_install.len()
        ));
        return Ok(TaskResult::DryRun);
    }

    let mut changed = 0u32;
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
            changed += 1;
        } else {
            ctx.log.info(&format!("{name} failed to install"));
        }
    }

    ctx.log
        .info(&format!("{changed} changed, {already} already ok"));
    Ok(TaskResult::Ok)
}
