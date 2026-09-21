//! Application self-update re-execution policy.

use anyhow::Result;

use crate::infra::logging::Output;

use super::{RuntimePolicy, runner};
use crate::infra::logging::OutputExt as _;

/// Environment variable set before re-exec to skip self-update and run-lock reacquisition.
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
    let args = log.run_log().map_or_else(
        || args.clone(),
        |run| crate::infra::logging::records::child_args(&args, &run.id()),
    );
    let exe = re_exec_path(root);
    let command = build_reexec_command(&exe, &args);
    run_reexec(command, log)
}

fn finish_reexec(log: &dyn Output, code: i32) {
    if let Some(run) = log.run_log() {
        use crate::infra::logging::records::RunOutcome;
        run.finish(
            match code {
                0 => RunOutcome::Succeeded,
                130 => RunOutcome::Interrupted,
                _ => RunOutcome::Failed,
            },
            code,
        );
    }
}

fn run_reexec(mut command: std::process::Command, log: &dyn Output) -> ! {
    match command.status() {
        Ok(status) => {
            if status.code().is_none() {
                log.warn("child process terminated by signal");
            }
            let code = status.code().unwrap_or(1);
            finish_reexec(log, code);
            std::process::exit(code)
        }
        Err(error) => {
            log.error(format!("failed to re-exec: {error}"));
            finish_reexec(log, 1);
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
            finish_reexec(log, 1);
            std::process::exit(1);
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = log.run_log().map_or_else(
        || args.clone(),
        |run| crate::infra::logging::records::child_args(&args, &run.id()),
    );
    let command = build_repository_reexec_command(&exe, &args);
    log.startup("Repository synced · restarting to load configuration");
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
/// Returns an error if the repository root cannot be resolved or the pre-update
/// check fails.
pub(crate) fn prepare_self_update(
    runtime: &RuntimePolicy<'_>,
    log: &std::sync::Arc<crate::infra::logging::Logger>,
) -> Result<Option<crate::infra::run_lock::RunLock>> {
    let run_lock = runner::CommandRunner::acquire_run_lock(runtime, log)?;
    if !should_check_self_update(runtime) {
        return Ok(run_lock);
    }

    let root = runner::resolve_root(runtime)?;
    if crate::domains::dotfiles::self_update::pre_update(
        &root,
        &**log,
        runtime.execution.dry_run,
        runtime.global.skip_attestation,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};

    #[test]
    fn parent_run_preserves_the_restarted_childs_outcome() {
        for (code, expected) in [
            (0, RunOutcome::Succeeded),
            (1, RunOutcome::Failed),
            (130, RunOutcome::Interrupted),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let log = crate::infra::logging::Logger::new_in("install", dir.path());
            let run = log.run_log().unwrap();
            run.start_run("install", None);

            finish_reexec(&log, code);

            let contents = std::fs::read_to_string(run.path()).unwrap();
            let finishes = contents
                .lines()
                .filter_map(StoredRecord::from_line)
                .filter_map(|stored| {
                    if let Record::RunFinish {
                        outcome, exit_code, ..
                    } = stored.record
                    {
                        Some((outcome, exit_code))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                finishes,
                [(expected, code)],
                "the parent must not label an interrupted child as failed"
            );
        }
    }
}
