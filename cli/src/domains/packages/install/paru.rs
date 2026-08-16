use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use crate::engine::Context;
use crate::infra::exec::{CommandSpec, Executor};
use crate::infra::logging::OutputExt as _;

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Result of resolving and executing the PATH-selected `paru` binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParuHealth {
    /// No `paru` executable was found on `PATH`.
    Missing { reason: String },
    /// The resolved executable successfully ran its non-destructive version check.
    Healthy { path: PathBuf, version: String },
    /// An executable was found, but the dynamic loader or process returned an error.
    Broken { path: PathBuf, reason: String },
}

/// Resolve `paru` on `PATH` and run that exact executable.
pub(super) fn check_paru_health(executor: &dyn Executor) -> ParuHealth {
    let path = match executor.which_path("paru") {
        Ok(path) => path,
        Err(error) => {
            return ParuHealth::Missing {
                reason: error.to_string(),
            };
        }
    };

    match executor.execute(CommandSpec::new(path.as_os_str()).arg("--version")) {
        Ok(result) => {
            let version = result
                .stdout
                .lines()
                .chain(result.stderr.lines())
                .find(|line| !line.trim().is_empty())
                .map_or_else(
                    || "version check passed".to_string(),
                    |line| line.trim().to_string(),
                );
            ParuHealth::Healthy { path, version }
        }
        Err(error) => ParuHealth::Broken {
            path,
            reason: error.to_string(),
        },
    }
}

/// Check that required tools are available for building paru.
pub(super) fn check_prerequisites(ctx: &Context) -> Result<()> {
    for dep in ["git", "makepkg", "sudo"] {
        if !ctx.executor().which(dep) {
            anyhow::bail!("missing prerequisite: {dep}");
        }
        ctx.debug_fmt(|| format!("prerequisite ok: {dep}"));
    }
    Ok(())
}

/// Prepare a clean build directory for paru.
pub(super) fn prepare_build_directory(ctx: &Context) -> Result<PathBuf> {
    let tmp = std::env::temp_dir().join("paru-build");
    if tmp.exists() {
        ctx.log().debug("removing previous paru build directory");
        std::fs::remove_dir_all(&tmp).context("removing previous paru build directory")?;
    }
    Ok(tmp)
}

/// Clone the source-built paru AUR package.
pub(super) fn clone_paru_from_aur(ctx: &Context, tmp: &Path) -> Result<()> {
    ctx.log().debug("cloning paru from AUR");
    ctx.executor()
        .execute(CommandSpec::new("git").args(&[
            "clone",
            "https://aur.archlinux.org/paru.git",
            &tmp.to_string_lossy(),
        ]))
        .context("cloning paru from AUR")?;
    Ok(())
}

/// Build paru using makepkg with parallel compilation.
pub(super) fn build_paru(ctx: &Context, tmp: &Path) -> Result<()> {
    let nproc = std::thread::available_parallelism()
        .map_or_else(|_| DEFAULT_NPROC.to_string(), |n| n.get().to_string());

    let makeflags = format!("-j{nproc}");
    ctx.log()
        .debug(format!("building with MAKEFLAGS={makeflags}"));
    ctx.executor()
        .execute(
            CommandSpec::new("makepkg")
                .args(&["-si", "--noconfirm"])
                .current_dir(tmp)
                .env("MAKEFLAGS", &makeflags),
        )
        .context("building paru with makepkg")?;
    Ok(())
}
