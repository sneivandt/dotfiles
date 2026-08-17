use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use crate::engine::Context;
use crate::infra::exec::{CommandSpec, Executor};
use crate::infra::logging::OutputExt as _;

use super::super::{PARU_EXECUTABLE, PARU_PACKAGE};

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

/// Result of validating the target system's installed `paru` package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParuHealth {
    /// The target package is absent and no stale executable was found on `PATH`.
    Missing { reason: String },
    /// The target package and its canonical executable passed validation.
    Healthy {
        path: PathBuf,
        package: String,
        version: String,
    },
    /// The target package or an executable visible on `PATH` is inconsistent or unusable.
    Broken { path: PathBuf, reason: String },
}

fn first_output_line(result: &crate::infra::exec::ExecResult) -> Option<&str> {
    result
        .stdout
        .lines()
        .chain(result.stderr.lines())
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
}

fn missing_or_stale(executor: &dyn Executor, reason: String) -> ParuHealth {
    match executor.which_path(PARU_PACKAGE) {
        Ok(path) => ParuHealth::Broken {
            path,
            reason: format!(
                "{reason}; a PATH-selected paru executable exists but is not backed by the target package database"
            ),
        },
        Err(_) => ParuHealth::Missing { reason },
    }
}

/// Query the current Arch package database and execute its canonical `paru` binary.
///
/// `install-arch` launches dotfiles through `arch-chroot /mnt`, so `pacman` and
/// `/usr/bin/paru` here both refer to the target system. The explicit path also
/// makes validation independent of the caller's PATH and is reused by later AUR
/// package operations.
pub(super) fn check_paru_health(executor: &dyn Executor) -> ParuHealth {
    let package_result = match executor.execute(
        CommandSpec::new("pacman")
            .args(&["-Q", PARU_PACKAGE])
            .unchecked(),
    ) {
        Ok(result) => result,
        Err(error) => {
            return missing_or_stale(
                executor,
                format!("could not query target package {PARU_PACKAGE}: {error}"),
            );
        }
    };
    if !package_result.success {
        let detail =
            first_output_line(&package_result).unwrap_or("package query returned no output");
        return missing_or_stale(
            executor,
            format!("target package {PARU_PACKAGE} is not installed: {detail}"),
        );
    }
    let package = first_output_line(&package_result)
        .map_or_else(|| PARU_PACKAGE.to_string(), ToString::to_string);
    let path = PathBuf::from(PARU_EXECUTABLE);

    match executor.execute(CommandSpec::new(path.as_os_str()).arg("--version")) {
        Ok(result) => {
            let version = first_output_line(&result)
                .map_or_else(|| "version check passed".to_string(), ToString::to_string);
            ParuHealth::Healthy {
                path,
                package,
                version,
            }
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

    let cargo = ctx.executor().which_path("cargo").with_context(|| {
        "missing prerequisite: cargo is not available; on Arch install the complete toolchain with `sudo pacman -Syu --needed rust`"
    })?;
    let result = ctx
        .executor()
        .execute(CommandSpec::new(cargo.as_os_str()).arg("--version"))
        .with_context(|| {
            format!(
                "Rust/Cargo prerequisite is incomplete: {} exists but `cargo --version` failed; on Arch install `rust` with `sudo pacman -Syu --needed rust`, or if rustup is intentional configure a toolchain with `rustup default stable`",
                cargo.display()
            )
        })?;
    let version = result
        .stdout
        .lines()
        .chain(result.stderr.lines())
        .find(|line| !line.trim().is_empty())
        .map_or("version check passed", str::trim);
    ctx.debug_fmt(|| format!("prerequisite ok: cargo · {version}"));
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
