//! Application self-update re-execution policy.

use anyhow::Result;

use crate::app::cli::GlobalOpts;
use crate::infra::logging::Output;

use super::runner;
use crate::infra::logging::OutputExt as _;

/// Environment variable set before re-exec so the child does not reacquire the run lock.
pub(super) const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";
/// Environment variable set when self-update replaced the running binary.
pub(super) const SELF_UPDATE_REEXEC_GUARD_VAR: &str = "DOTFILES_SELF_UPDATE_REEXEC_GUARD";
/// Environment variable set when a repository refresh caused the re-exec.
pub(super) const REPOSITORY_REEXEC_GUARD_VAR: &str = "DOTFILES_REPOSITORY_REEXEC_GUARD";

/// Replace the current process with a fresh invocation of the same binary.
///
/// The updated binary is spawned as a child that inherits stdio and is waited
/// on, so the parent retains the repository run lock until the replacement
/// process finishes.
pub(crate) fn re_exec(root: &std::path::Path, log: &dyn Output) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = re_exec_path(root);
    let command = build_reexec_command(&exe, &args);
    run_reexec(command, log)
}

fn run_reexec(mut command: std::process::Command, log: &dyn Output) -> ! {
    match command.status() {
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

/// Build the replacement process while preserving the original CLI arguments.
pub(super) fn build_reexec_command(
    exe: &std::path::Path,
    args: &[String],
) -> std::process::Command {
    let mut command = build_guarded_reexec_command(exe, args);
    command.env(SELF_UPDATE_REEXEC_GUARD_VAR, "1");
    command
}

fn build_guarded_reexec_command(exe: &std::path::Path, args: &[String]) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(args).env(REEXEC_GUARD_VAR, "1");
    command
}

/// Restart the current binary after the repository checkout changed.
pub(crate) fn re_exec_after_repository_update(log: &dyn Output) -> ! {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            log.error(format!(
                "failed to determine executable for repository restart: {error}"
            ));
            std::process::exit(1);
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = build_repository_reexec_command(&exe, &args);
    log.startup("Repository updated · restarting with refreshed configuration");
    run_reexec(command, log)
}

pub(super) fn build_repository_reexec_command(
    exe: &std::path::Path,
    args: &[String],
) -> std::process::Command {
    let mut command = build_guarded_reexec_command(exe, args);
    command.env(REPOSITORY_REEXEC_GUARD_VAR, "1");
    command
}

/// Return whether this process is the refreshed child of a repository update.
#[must_use]
pub(crate) fn repository_reexec_active(env: &dyn crate::infra::env::Env) -> bool {
    env.var_os(REPOSITORY_REEXEC_GUARD_VAR).is_some()
}

pub(super) fn self_update_check_policy(
    env: &dyn crate::infra::env::Env,
    elevated: bool,
) -> Option<crate::domains::dotfiles::self_update::CachePolicy> {
    let repository_child = repository_reexec_active(env);
    let guarded_non_repository_child = env.var_os(REEXEC_GUARD_VAR).is_some() && !repository_child;
    if elevated
        || env.var_os(SELF_UPDATE_REEXEC_GUARD_VAR).is_some()
        || guarded_non_repository_child
    {
        return None;
    }

    Some(if repository_child {
        crate::domains::dotfiles::self_update::CachePolicy::Refresh
    } else {
        crate::domains::dotfiles::self_update::CachePolicy::Use
    })
}

/// Run the shared self-update preflight and re-exec if the binary changed.
///
/// # Errors
///
/// Returns an error if the repository root cannot be resolved or the pre-update
/// check fails.
pub(crate) fn prepare_self_update(
    global: &GlobalOpts,
    log: &std::sync::Arc<crate::infra::logging::Logger>,
) -> Result<Option<crate::infra::run_lock::RunLock>> {
    let run_lock = runner::CommandRunner::acquire_run_lock(global, log)?;
    let env = crate::infra::env::system();
    let Some(cache_policy) =
        self_update_check_policy(env.as_ref(), crate::infra::elevation::is_elevated_child())
    else {
        return Ok(run_lock);
    };

    let root = runner::resolve_root(global)?;
    if crate::domains::dotfiles::self_update::pre_update(
        &root,
        &**log,
        global.dry_run,
        global.skip_attestation,
        cache_policy,
    )? {
        re_exec(&root, &**log);
    }
    Ok(run_lock)
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
