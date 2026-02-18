use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Partition package names by installed status and handle dry-run reporting.
///
/// Checks each name against the `is_installed` predicate, logging already-installed
/// packages and collecting those that still need installation. In dry-run mode
/// each pending package is logged and `stats.changed` is set.
///
/// Returns `(stats, to_install)`.
fn partition_packages<'a, F>(
    names: &[&'a str],
    is_installed: F,
    ctx: &Context,
    label: &str,
) -> (TaskStats, Vec<&'a str>)
where
    F: Fn(&str) -> bool,
{
    let mut stats = TaskStats::new();
    let mut to_install = Vec::new();
    for name in names {
        if is_installed(name) {
            ctx.log.debug(&format!("ok: {name} (already installed)"));
            stats.already_ok += 1;
        } else {
            to_install.push(*name);
        }
    }
    if ctx.dry_run {
        for name in &to_install {
            ctx.log.dry_run(&format!("would install{label}: {name}"));
        }
        stats.changed = to_install.len() as u32;
    }
    (stats, to_install)
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
        ctx.platform.supports_aur() && ctx.config.packages.iter().any(|p| p.is_aur)
    }

    fn dependencies(&self) -> Vec<&str> {
        vec!["Install paru"]
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

        let (mut stats, to_install) =
            partition_packages(&names, |n| installed.contains(n), ctx, " (AUR)");

        if ctx.dry_run || to_install.is_empty() {
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

    let (mut stats, to_install) = partition_packages(&names, |n| installed.contains(n), ctx, "");

    if ctx.dry_run || to_install.is_empty() {
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

    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let (mut stats, to_install) = partition_packages(&names, |n| list.contains(n), ctx, "");

    if ctx.dry_run || to_install.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::config::Config;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::logging::Logger;
    use crate::platform::{Os, Platform};

    /// Build a minimal Context for unit-testing helpers.
    fn test_ctx(log: &Logger, dry_run: bool) -> Context {
        let config = Config {
            root: std::path::PathBuf::from("/tmp/dotfiles-test"),
            profile: Profile {
                name: "base".to_string(),
                active_categories: vec!["base".to_string()],
                excluded_categories: Vec::new(),
            },
            packages: Vec::new(),
            symlinks: Vec::new(),
            registry: Vec::new(),
            units: Vec::new(),
            chmod: Vec::new(),
            vscode_extensions: Vec::new(),
            copilot_skills: Vec::new(),
            manifest: Manifest {
                excluded_files: Vec::new(),
            },
        };
        let platform = Platform::new(Os::Linux, false);
        // Leak references so they outlive the function — acceptable in tests
        let config = Box::leak(Box::new(config));
        let platform = Box::leak(Box::new(platform));
        Context {
            config,
            platform,
            log,
            dry_run,
            home: std::path::PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn partition_all_installed() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, false);
        let installed: HashSet<String> = ["git", "vim"].iter().map(|s| s.to_string()).collect();
        let names = vec!["git", "vim"];

        let (stats, to_install) = partition_packages(&names, |n| installed.contains(n), &ctx, "");
        assert_eq!(stats.already_ok, 2);
        assert!(to_install.is_empty());
    }

    #[test]
    fn partition_some_missing() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, false);
        let installed: HashSet<String> = ["git"].iter().map(|s| s.to_string()).collect();
        let names = vec!["git", "vim", "curl"];

        let (stats, to_install) = partition_packages(&names, |n| installed.contains(n), &ctx, "");
        assert_eq!(stats.already_ok, 1);
        assert_eq!(to_install, vec!["vim", "curl"]);
    }

    #[test]
    fn partition_dry_run_sets_changed() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, true);
        let installed: HashSet<String> = HashSet::new();
        let names = vec!["git", "vim"];

        let (stats, to_install) = partition_packages(&names, |n| installed.contains(n), &ctx, "");
        assert_eq!(stats.changed, 2);
        assert_eq!(to_install.len(), 2);
    }
}
