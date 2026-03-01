//! Tasks: install system packages.
use anyhow::{Context as _, Result};

use super::{
    Context, ProcessOpts, Task, TaskResult, TaskStats, process_resource_states, task_deps,
};
use crate::config::packages::Package;
use crate::resources::Applicable as _;
use crate::resources::package::{
    PackageManager, PackageResource, batch_install_packages, get_installed_packages,
};

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Process a list of packages using batch-checked installed state.
///
/// Queries all installed packages **once**, then installs all missing packages:
/// - **Pacman / Paru**: a single batch command (`pacman -S --needed …`) for all
///   missing packages, letting the package manager resolve dependencies across
///   the full set at once.
/// - **Winget**: individual installs per package so that one failure does not
///   prevent the remaining packages from being attempted.
fn process_packages(
    ctx: &Context,
    packages: &[Package],
    manager: PackageManager,
) -> Result<TaskResult> {
    ctx.log.debug(&format!(
        "batch-checking {} packages with a single query",
        packages.len()
    ));
    let installed = get_installed_packages(manager, &*ctx.executor)?;

    // Winget does not support multi-package installs; fall back to the
    // per-resource loop so a single failure does not abort the rest.
    if manager == PackageManager::Winget {
        let resource_states = packages.iter().map(|pkg| {
            let resource = PackageResource::new(pkg.name.clone(), manager, &*ctx.executor);
            let state = resource.state_from_installed(&installed);
            (resource, state)
        });
        return process_resource_states(
            ctx,
            resource_states,
            &ProcessOpts::apply_all("install").no_bail(),
        );
    }

    // Pacman / Paru: collect missing packages and install them in one batch
    // command, which is faster and resolves cross-package dependencies once.
    let resources: Vec<PackageResource> = packages
        .iter()
        .map(|pkg| PackageResource::new(pkg.name.clone(), manager, &*ctx.executor))
        .collect();

    let mut stats = TaskStats::new();
    let mut missing = Vec::new();

    for r in &resources {
        if installed.contains(&r.name) {
            ctx.log.debug(&format!("ok: {}", r.description()));
            stats.already_ok += 1;
        } else {
            missing.push(r);
        }
    }

    if missing.is_empty() {
        return Ok(stats.finish(ctx));
    }

    if ctx.dry_run {
        for r in &missing {
            ctx.log
                .dry_run(&format!("would install: {}", r.description()));
        }
        stats.changed = missing.len() as u32;
        return Ok(stats.finish(ctx));
    }

    ctx.log
        .debug(&format!("batch-installing {} packages", missing.len()));
    if let Err(e) = batch_install_packages(&missing) {
        ctx.log.warn(&format!("batch install failed: {e:#}"));
        stats.skipped = missing.len() as u32;
    } else {
        stats.changed = missing.len() as u32;
    }

    Ok(stats.finish(ctx))
}

/// Install system packages via pacman or winget.
#[derive(Debug)]
pub struct InstallPackages;

impl Task for InstallPackages {
    fn name(&self) -> &'static str {
        "Install packages"
    }

    task_deps![super::reload_config::ReloadConfig];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().packages.iter().any(|p| !p.is_aur)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages: Vec<Package> = ctx
            .config_read()
            .packages
            .iter()
            .filter(|p| !p.is_aur)
            .cloned()
            .collect();

        if packages.is_empty() {
            return Ok(TaskResult::Skipped("no packages to install".to_string()));
        }

        ctx.log
            .debug(&format!("{} non-AUR packages to process", packages.len()));

        let manager = if ctx.platform.is_linux() {
            ctx.log.debug("using pacman package manager");
            if !ctx.executor.which("pacman") {
                return Ok(TaskResult::Skipped("pacman not found".to_string()));
            }
            PackageManager::Pacman
        } else {
            ctx.log.debug("using winget package manager");
            if !ctx.executor.which("winget") {
                return Ok(TaskResult::Skipped("winget not found".to_string()));
            }
            PackageManager::Winget
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

    task_deps![InstallParu, super::reload_config::ReloadConfig];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_aur() && ctx.config_read().packages.iter().any(|p| p.is_aur)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let packages: Vec<Package> = ctx
            .config_read()
            .packages
            .iter()
            .filter(|p| p.is_aur)
            .cloned()
            .collect();

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

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.uses_pacman()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.executor.which("paru") {
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
    for dep in ["git", "makepkg", "sudo"] {
        if !ctx.executor.which(dep) {
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

/// Remove the build directory (best effort, logs a warning on failure).
fn cleanup_build_directory(tmp: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(tmp) {
        tracing::warn!(
            "failed to remove paru build directory {}: {e}",
            tmp.display()
        );
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    use crate::config::packages::Package;
    use crate::platform::Os;
    use crate::resources::package::PackageResource;
    use crate::tasks::test_helpers::{
        empty_config, make_arch_context, make_linux_context, make_platform_context_with_which,
        make_windows_context,
    };
    use std::path::PathBuf;

    #[test]
    fn package_resource_description() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.description(), "git (pacman)");

        let resource =
            PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.description(), "Git.Git (winget)");
    }

    // -----------------------------------------------------------------------
    // InstallPackages::should_run
    // -----------------------------------------------------------------------

    #[test]
    fn install_packages_should_run_false_when_no_packages() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!InstallPackages.should_run(&ctx));
    }

    #[test]
    fn install_packages_should_run_false_when_only_aur_packages() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "paru-bin".to_string(),
            is_aur: true,
        });
        let ctx = make_arch_context(config);
        assert!(!InstallPackages.should_run(&ctx));
    }

    #[test]
    fn install_packages_should_run_true_when_non_aur_packages_present() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        let ctx = make_linux_context(config);
        assert!(InstallPackages.should_run(&ctx));
    }

    // -----------------------------------------------------------------------
    // InstallAurPackages::should_run
    // -----------------------------------------------------------------------

    #[test]
    fn install_aur_packages_should_run_false_on_non_arch() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "paru-bin".to_string(),
            is_aur: true,
        });
        let ctx = make_linux_context(config); // not arch
        assert!(!InstallAurPackages.should_run(&ctx));
    }

    #[test]
    fn install_aur_packages_should_run_false_when_no_aur_packages() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        let ctx = make_arch_context(config);
        assert!(!InstallAurPackages.should_run(&ctx));
    }

    #[test]
    fn install_aur_packages_should_run_true_on_arch_with_aur_packages() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "paru-bin".to_string(),
            is_aur: true,
        });
        let ctx = make_arch_context(config);
        assert!(InstallAurPackages.should_run(&ctx));
    }

    // -----------------------------------------------------------------------
    // InstallParu::should_run
    // -----------------------------------------------------------------------

    #[test]
    fn install_paru_should_run_false_on_non_arch_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!InstallParu.should_run(&ctx));
    }

    #[test]
    fn install_paru_should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!InstallParu.should_run(&ctx));
    }

    #[test]
    fn install_paru_should_run_true_on_arch_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_arch_context(config);
        assert!(InstallParu.should_run(&ctx));
    }

    // -----------------------------------------------------------------------
    // run() — early-exit paths that do not require a real package manager
    // -----------------------------------------------------------------------

    #[test]
    fn install_packages_run_skips_when_pacman_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        // which_result=false ⇒ pacman not found
        let ctx = make_platform_context_with_which(config, Os::Linux, false, false);
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("pacman not found")),
            "expected 'pacman not found' skip, got {result:?}"
        );
    }

    #[test]
    fn install_packages_run_skips_when_winget_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "Git.Git".to_string(),
            is_aur: false,
        });
        // which_result=false ⇒ winget not found
        let ctx = make_platform_context_with_which(config, Os::Windows, false, false);
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("winget not found")),
            "expected 'winget not found' skip, got {result:?}"
        );
    }

    #[test]
    fn install_paru_run_returns_ok_when_already_installed() {
        let config = empty_config(PathBuf::from("/tmp"));
        // which_result=true ⇒ paru found in PATH
        let ctx = make_platform_context_with_which(config, Os::Linux, true, true);
        let result = InstallParu.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok when paru already installed, got {result:?}"
        );
    }

    #[test]
    fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
        let config = empty_config(PathBuf::from("/tmp"));
        // which_result=true ⇒ paru found in PATH
        let mut ctx = make_platform_context_with_which(config, Os::Linux, true, true);
        ctx.dry_run = true;
        let result = InstallParu.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok when paru already installed in dry-run mode (no change needed), got {result:?}"
        );
    }

    #[test]
    fn install_aur_packages_run_skips_when_paru_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "paru-bin".to_string(),
            is_aur: true,
        });
        // which_result=false ⇒ paru not found
        let ctx = make_platform_context_with_which(config, Os::Linux, true, false);
        let result = InstallAurPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("paru not installed")),
            "expected 'paru not installed' skip, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run() — batch install paths (pacman/paru)
    // -----------------------------------------------------------------------

    /// Build a context that uses a [`MockExecutor`] with `which=true`.
    ///
    /// This lets tests exercise the `process_packages` batch install path without
    /// being short-circuited by the "tool not found" guard in `run()`.
    fn make_package_context(
        config: crate::config::Config,
        os: Os,
        is_arch: bool,
        responses: Vec<(bool, String)>,
    ) -> crate::tasks::Context {
        use crate::platform::Platform;
        use crate::resources::test_helpers::MockExecutor;
        crate::tasks::test_helpers::make_context(
            config,
            std::sync::Arc::new(Platform::new(os, is_arch)),
            std::sync::Arc::new(MockExecutor::with_responses(responses).with_which(true)),
        )
    }

    #[test]
    fn install_packages_batch_installs_missing_packages_on_arch() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        config.packages.push(Package {
            name: "vim".to_string(),
            is_aur: false,
        });
        // Response 1: pacman -Q → vim is installed, git is not
        // Response 2: sudo pacman -S --needed --noconfirm git → success
        let ctx = make_package_context(
            config,
            Os::Linux,
            true, // Arch Linux
            vec![(true, "vim 9.0\n".to_string()), (true, String::new())],
        );
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok after batch install, got {result:?}"
        );
    }

    #[test]
    fn install_packages_all_already_installed_returns_ok() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        // Response: pacman -Q → git is installed, no install command expected
        let ctx = make_package_context(
            config,
            Os::Linux,
            false, // plain Linux (also uses pacman path)
            vec![(true, "git 2.40\n".to_string())],
        );
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok when all packages already installed, got {result:?}"
        );
    }

    #[test]
    fn install_packages_dry_run_reports_missing_packages() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "git".to_string(),
            is_aur: false,
        });
        // Response: pacman -Q → nothing installed
        let mut ctx = make_package_context(config, Os::Linux, true, vec![(true, String::new())]);
        ctx.dry_run = true;
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun, got {result:?}"
        );
    }

    #[test]
    fn install_packages_winget_installs_per_package() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.packages.push(Package {
            name: "Git.Git".to_string(),
            is_aur: false,
        });
        // Response 1: winget list (get_installed_packages) → empty (nothing installed)
        // Response 2: winget install Git.Git (apply via process_resource_states) → success
        let ctx = make_package_context(
            config,
            Os::Windows,
            false,
            vec![(true, String::new()), (true, String::new())],
        );
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok after winget per-package install, got {result:?}"
        );
    }
}
