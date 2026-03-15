//! Tasks: install system packages.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::config::packages::Package;
use crate::resources::Applicable as _;
use crate::resources::package::{
    PackageManager, PackageResource, batch_install_packages, get_installed_packages,
};
use crate::tasks::{
    Context, ProcessOpts, Task, TaskPhase, TaskResult, TaskStats, process_resource_states,
    task_deps,
};

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Install system packages via pacman or winget.
#[derive(Debug)]
pub struct InstallPackages;

impl Task for InstallPackages {
    fn name(&self) -> &'static str {
        "Install packages"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::User
    }

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

    fn phase(&self) -> TaskPhase {
        TaskPhase::User
    }

    task_deps![InstallParu];

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

    fn phase(&self) -> TaskPhase {
        TaskPhase::User
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
// Package installation strategies
// ---------------------------------------------------------------------------

/// Strategy for installing packages within a single package-manager scope.
///
/// Implementations decide **how** missing packages are installed (one-by-one
/// vs. a single batch command) while [`process_packages`] handles the shared
/// query-installed-first-then-delegate workflow.
trait PackageStrategy {
    /// Install the given packages, using `installed` to skip already-present
    /// packages.
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &HashSet<String>,
    ) -> Result<TaskResult>;
}

/// Batch strategy (Pacman / Paru): collect all missing packages and install
/// them in **one** package-manager invocation.  This is faster and lets the
/// solver resolve cross-package dependencies across the full set.
struct BatchInstall {
    manager: PackageManager,
}

impl PackageStrategy for BatchInstall {
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &HashSet<String>,
    ) -> Result<TaskResult> {
        let resources: Vec<PackageResource> = packages
            .iter()
            .map(|pkg| {
                PackageResource::new(pkg.name.clone(), self.manager, Arc::clone(&ctx.executor))
            })
            .collect();

        let mut stats = TaskStats::new();
        let mut missing = Vec::new();

        for r in &resources {
            if installed.contains(&r.name) {
                ctx.debug_fmt(|| format!("ok: {}", r.description()));
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
}

/// Individual strategy (Winget): install each package separately so that one
/// failure does not prevent the remainder from being attempted.
struct IndividualInstall {
    manager: PackageManager,
}

impl PackageStrategy for IndividualInstall {
    fn install(
        &self,
        ctx: &Context,
        packages: &[Package],
        installed: &HashSet<String>,
    ) -> Result<TaskResult> {
        let resource_states = packages.iter().map(|pkg| {
            let resource =
                PackageResource::new(pkg.name.clone(), self.manager, Arc::clone(&ctx.executor));
            let state = resource.state_from_installed(installed);
            (resource, state)
        });
        process_resource_states(ctx, resource_states, &ProcessOpts::lenient("install"))
    }
}

/// Process a list of packages using the appropriate strategy for the given
/// package manager.
///
/// Queries all installed packages **once**, then delegates to either
/// [`BatchInstall`] (Pacman / Paru) or [`IndividualInstall`] (Winget).
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

    let strategy: &dyn PackageStrategy = match manager {
        PackageManager::Winget => &IndividualInstall { manager },
        _ => &BatchInstall { manager },
    };
    strategy.install(ctx, packages, &installed)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::config::packages::Package;
    use crate::exec::Executor;
    use crate::platform::Os;
    use crate::resources::Applicable;
    use crate::resources::package::{PackageManager, PackageResource};
    use crate::tasks::test_helpers::{
        empty_config, make_arch_context, make_linux_context, make_platform_context_with_which,
        make_windows_context,
    };
    use std::path::PathBuf;

    #[test]
    fn package_resource_description() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "git (pacman)");

        let resource = PackageResource::new(
            "paru-bin".to_string(),
            PackageManager::Paru,
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource = PackageResource::new(
            "Git.Git".to_string(),
            PackageManager::Winget,
            Arc::clone(&executor),
        );
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
        ctx = ctx.with_dry_run(true);
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

    use crate::exec::{ExecResult, MockExecutor};

    fn ok_result(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    /// Build a context that uses a [`MockExecutor`] with `which=true`.
    ///
    /// This lets tests exercise the `process_packages` batch install path without
    /// being short-circuited by the "tool not found" guard in `run()`.
    fn make_package_context(
        config: crate::config::Config,
        os: Os,
        is_arch: bool,
        executor: MockExecutor,
    ) -> crate::tasks::Context {
        use crate::platform::Platform;
        crate::tasks::test_helpers::make_context(
            config,
            Platform::new(os, is_arch),
            std::sync::Arc::new(executor),
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
        // which("pacman") → true
        // run_unchecked("pacman", ["-Q"]) → vim installed, git not
        // run("sudo", ["pacman", "-S", "--needed", "--noconfirm", "git"]) → success
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(|_| true);
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result("vim 9.0\n")));
        mock.expect_run()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result("")));
        let ctx = make_package_context(config, Os::Linux, true, mock);
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
        // which("pacman") → true
        // run_unchecked("pacman", ["-Q"]) → git installed → no install needed
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(|_| true);
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(ok_result("git 2.40\n")));
        let ctx = make_package_context(config, Os::Linux, false, mock);
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
        // which("pacman") → true
        // run_unchecked("pacman", ["-Q"]) → nothing installed
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(|_| true);
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(ok_result("")));
        let mut ctx = make_package_context(config, Os::Linux, true, mock);
        ctx = ctx.with_dry_run(true);
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
        // which("winget") → true
        // run_unchecked("winget", ["list", ...]) → empty (nothing installed)
        // run_unchecked("winget", ["install", ...]) → success
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(|_| true);
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result("")));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result("")));
        let ctx = make_package_context(config, Os::Windows, false, mock);
        let result = InstallPackages.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok after winget per-package install, got {result:?}"
        );
    }
}
