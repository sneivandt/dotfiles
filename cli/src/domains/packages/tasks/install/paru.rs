use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use crate::engine::Context;

/// Default number of parallel jobs for makepkg if nproc detection fails.
const DEFAULT_NPROC: &str = "4";

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

/// Clone the paru-bin AUR package.
pub(super) fn clone_paru_from_aur(ctx: &Context, tmp: &Path) -> Result<()> {
    ctx.log().debug("cloning paru-bin from AUR");
    ctx.executor()
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
pub(super) fn build_paru(ctx: &Context, tmp: &Path) -> Result<()> {
    let nproc = std::thread::available_parallelism()
        .map_or_else(|_| DEFAULT_NPROC.to_string(), |n| n.get().to_string());

    let makeflags = format!("-j{nproc}");
    ctx.log()
        .debug(&format!("building with MAKEFLAGS={makeflags}"));
    ctx.executor()
        .run_in_with_env(
            tmp,
            "makepkg",
            &["-si", "--noconfirm"],
            &[("MAKEFLAGS", &makeflags)],
        )
        .context("building paru with makepkg")?;
    Ok(())
}
