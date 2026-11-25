//! Application self-update re-execution policy.

use anyhow::{Context as _, Result};

use crate::infra::logging::Output;
use crate::infra::run_lock::CommandLocks;

use super::RuntimePolicy;
use crate::infra::logging::OutputExt as _;

/// Environment variable set before re-exec to skip self-update and run-lock reacquisition.
pub(super) const REEXEC_GUARD_VAR: &str = "DOTFILES_REEXEC_GUARD";
/// Environment variable set when self-update replaced the running binary.
pub(super) const SELF_UPDATE_REEXEC_GUARD_VAR: &str = "DOTFILES_SELF_UPDATE_REEXEC_GUARD";
/// Environment variable set when a repository refresh caused the re-exec.
pub(super) const REPOSITORY_REEXEC_GUARD_VAR: &str = "DOTFILES_REPOSITORY_REEXEC_GUARD";
pub(super) const INVOCATION_START_VAR: &str = "DOTFILES_INVOCATION_START_US";
pub(super) const STARTUP_TRANSITION_VAR: &str = "DOTFILES_STARTUP_TRANSITION";

/// Replace the current process with a fresh invocation of the same binary.
///
/// The updated binary is spawned as a child that inherits stdio and is waited
/// on, so the parent retains the repository run lock until the replacement
/// process finishes.
fn re_exec(
    root: &std::path::Path,
    locks: Option<&CommandLocks>,
    log: &crate::infra::logging::Logger,
) -> ! {
    let command = build_reexec_command(
        &re_exec_path(root),
        &reexec_args(log),
        SELF_UPDATE_REEXEC_GUARD_VAR,
    );
    run_reexec(command, root, locks, log)
}

fn reexec_args(log: &dyn Output) -> Vec<std::ffi::OsString> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(id) = log.run_id() {
        crate::infra::logging::records::child_args(&args, &id)
    } else {
        args
    }
}

fn finish_reexec(log: &dyn Output, code: i32) -> i32 {
    log.finish_run(code)
}

fn run_reexec(
    mut command: std::process::Command,
    root: &std::path::Path,
    locks: Option<&CommandLocks>,
    log: &crate::infra::logging::Logger,
) -> ! {
    let mut setup = || -> Result<_> {
        command.env(
            INVOCATION_START_VAR,
            log.invocation_start()
                .duration_since(std::time::UNIX_EPOCH)
                .context("invocation start precedes the Unix epoch")?
                .as_micros()
                .to_string(),
        );
        // Each child verifies the immediate parent, which may own locks that
        // its predecessor did not acquire.
        let (continuation, mode) = locks
            .map(CommandLocks::prepare_restart)
            .transpose()?
            .unwrap_or((Vec::new(), crate::infra::run_lock::RestartLockMode::Legacy));
        let env = crate::infra::env::system();
        let lease = crate::infra::run_lock::RestartLease::new(
            root,
            env.as_ref(),
            crate::infra::platform::Platform::detect(),
            mode,
        )?;
        command.env(REEXEC_GUARD_VAR, lease.marker()?);
        Ok((lease, continuation))
    };
    let lease = match setup() {
        Ok(lease) => lease,
        Err(error) => {
            log.error(format!("could not prepare restart: {error:#}"));
            std::process::exit(finish_reexec(log, 1));
        }
    };
    if !crate::app::interrupt::suspend_for_restart() {
        log.warn("restart cancelled by interrupt");
        std::process::exit(finish_reexec(log, 130));
    }
    // Clearing the active footer also prevents the ticker from reviving it.
    log.clear_status_line();
    command.env_remove(STARTUP_TRANSITION_VAR);
    if let Some(message) = log.startup_transition() {
        command.env(STARTUP_TRANSITION_VAR, message);
    }
    let code = match command.status() {
        Ok(status) => {
            drop(log.take_startup_transition());
            if status.code().is_none() {
                log.warn("child process terminated by signal");
            }
            status.code().unwrap_or(1)
        }
        Err(error) => {
            log.error(format!("failed to re-exec: {error}"));
            1
        }
    };
    drop(lease);
    std::process::exit(finish_reexec(log, code))
}

/// Build the replacement process while preserving the original CLI arguments.
pub(super) fn build_reexec_command(
    exe: &std::path::Path,
    args: &[std::ffi::OsString],
    guard: &str,
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .env(REEXEC_GUARD_VAR, "1")
        .env_remove(REPOSITORY_OVERLAY_LOCK_VAR)
        .env(guard, "1");
    command
}

/// Restart the current binary after the repository checkout changed.
pub(crate) fn re_exec_after_repository_update(
    root: &std::path::Path,
    profile: &str,
    overlay: Option<&std::path::Path>,
    exclusions: &str,
    locks: Option<&CommandLocks>,
    log: &crate::infra::logging::Logger,
) -> ! {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            log.error(format!(
                "failed to determine executable for repository restart: {error}"
            ));
            std::process::exit(finish_reexec(log, 1));
        }
    };
    let mut command = build_reexec_command(&exe, &reexec_args(log), REPOSITORY_REEXEC_GUARD_VAR);
    command
        .env(REPOSITORY_PROFILE_VAR, profile)
        .env(
            REPOSITORY_OVERLAY_VAR,
            overlay.unwrap_or_else(|| std::path::Path::new("")),
        )
        .env(REPOSITORY_OVERLAY_LOCK_VAR, "1")
        .env(REPOSITORY_EXCLUSIONS_VAR, exclusions);
    log.transition("Repository synced · restarting to load configuration");
    run_reexec(command, root, locks, log)
}

pub(super) const REPOSITORY_PROFILE_VAR: &str = "DOTFILES_RESTART_PROFILE";
pub(super) const REPOSITORY_OVERLAY_VAR: &str = "DOTFILES_RESTART_OVERLAY";
pub(super) const REPOSITORY_OVERLAY_LOCK_VAR: &str = "DOTFILES_RESTART_OVERLAY_LOCKED";
pub(super) const REPOSITORY_EXCLUSIONS_VAR: &str = "DOTFILES_RESTART_EXCLUSIONS";

pub(super) const fn should_check_self_update(runtime: &RuntimePolicy<'_>) -> bool {
    !runtime.execution.elevated_child
        && !runtime.self_update_child
        && !runtime.reexec_guarded
        && !runtime.repository_child
}

/// Run the shared self-update preflight and re-exec if the binary changed.
///
/// # Errors
///
/// Returns an error if the pre-update check fails.
pub(super) fn prepare_self_update(
    runtime: &RuntimePolicy<'_>,
    root: &std::path::Path,
    locks: Option<&CommandLocks>,
    log: &crate::infra::logging::Logger,
) -> Result<()> {
    if let Some(notice) = crate::domains::dotfiles::self_update::pre_update(
        root,
        log,
        runtime.execution.dry_run,
        runtime.global.skip_attestation,
    )? {
        log.queue_startup_transition(notice);
        re_exec(root, locks, log);
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

#[cfg(test)]
#[path = "tests/reexec.rs"]
mod tests;
