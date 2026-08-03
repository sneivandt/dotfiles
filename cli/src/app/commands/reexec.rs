//! Application self-update re-execution policy.

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::infra::logging::Output;

use super::runner;
use crate::infra::logging::OutputExt as _;

/// Environment variable set before re-exec to prevent infinite self-update loops.
pub(super) const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";

/// Replace the current process with a fresh invocation of the same binary.
///
/// On Unix the process image is replaced outright.  Elsewhere the updated
/// binary is spawned as a child that inherits stdio and waited on, so its
/// output stays sequential with the parent's and the shell prompt is only
/// redrawn once everything has finished.
pub(crate) fn re_exec(root: &std::path::Path, log: &dyn Output) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = re_exec_path(root);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let err = std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .exec();
        log.error(format!("failed to re-exec: {err}"));
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        match std::process::Command::new(&exe)
            .args(&args)
            .env(REEXEC_GUARD_VAR, "1")
            .status()
        {
            Ok(status) => {
                if status.code().is_none() {
                    log.warn("child process terminated by signal");
                }
                std::process::exit(status.code().unwrap_or(1))
            }
            Err(error) => {
                log.error(format!("failed to re-exec: {error}"));
                std::process::exit(1);
            }
        }
    }
}

/// Run the shared self-update preflight and re-exec if the binary changed.
///
/// # Errors
///
/// Returns an error if the repository root cannot be resolved or the pre-update
/// check fails.
pub(crate) fn prepare_self_update(global: &GlobalOpts, log: &dyn Output) -> Result<()> {
    let root = runner::resolve_root(global)?;
    if std::env::var_os(REEXEC_GUARD_VAR).is_none()
        && crate::domains::dotfiles::self_update::pre_update(&root, log, global.dry_run)?
    {
        re_exec(&root, log);
    }
    Ok(())
}

/// Path of the freshly installed binary to re-exec.
///
/// Derived from the repository root rather than [`std::env::current_exe`]:
/// self-update renames the running image out of the way, and on Windows
/// `current_exe` still reports the load-time path.  Re-exec only runs after
/// `pre_update` confirmed the process was launched from `<root>/bin`, so this
/// is always the binary that was just replaced.
pub(super) fn re_exec_path(root: &std::path::Path) -> std::path::PathBuf {
    crate::domains::dotfiles::self_update::installed_binary_path(root)
}
