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
    let command = build_reexec_command(
        &re_exec_path(root),
        &reexec_args(log),
        SELF_UPDATE_REEXEC_GUARD_VAR,
    );
    run_reexec(command, log)
}

fn reexec_args(log: &dyn Output) -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(id) = log.run_id() {
        crate::infra::logging::records::child_args(&args, &id)
    } else {
        args
    }
}

fn finish_reexec(log: &dyn Output, code: i32) -> i32 {
    log.finish_run(code)
}

fn run_reexec(mut command: std::process::Command, log: &dyn Output) -> ! {
    let code = match command.status() {
        Ok(status) => {
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
    std::process::exit(finish_reexec(log, code))
}

/// Build the replacement process while preserving the original CLI arguments.
pub(super) fn build_reexec_command(
    exe: &std::path::Path,
    args: &[String],
    guard: &str,
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .env(REEXEC_GUARD_VAR, "1")
        .env(guard, "1");
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
            std::process::exit(finish_reexec(log, 1));
        }
    };
    let command = build_reexec_command(&exe, &reexec_args(log), REPOSITORY_REEXEC_GUARD_VAR);
    log.startup("Repository synced · restarting to load configuration");
    run_reexec(command, log)
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
    use crate::infra::exec::{CommandSpec, Executor as _, ProcessExecutor};
    use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};

    const CHILD_TEST: &str = "app::commands::reexec::tests::restart_child";
    const CHILD_ROLE: &str = "DOTFILES_RESTART_TEST_ROLE";
    const CHILD_ROOT: &str = "DOTFILES_RESTART_TEST_ROOT";

    #[test]
    #[ignore = "subprocess fixture launched by restart outcome tests"]
    fn restart_child() {
        let role = std::env::var(CHILD_ROLE).unwrap();
        if let Some(code) = role.strip_prefix("exit-") {
            std::process::exit(code.parse().unwrap());
        }
        #[cfg(unix)]
        if role == "signal" {
            nix::sys::signal::raise(nix::sys::signal::Signal::SIGTERM).unwrap();
            panic!("SIGTERM must terminate the replacement process");
        }

        let root = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
        let log = crate::infra::logging::Logger::new_in("update", &root);
        let run = log.run_log().unwrap();
        run.start_run("update", None);
        std::fs::write(root.join("run-path"), run.path().to_str().unwrap()).unwrap();
        let mut command = if role == "missing" {
            std::process::Command::new(root.join("missing-executable"))
        } else {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
                .env(CHILD_ROLE, role.strip_prefix("parent-").unwrap());
            command
        };
        command.stdin(std::process::Stdio::null());
        run_reexec(command, &log);
    }

    #[test]
    fn restarted_process_exit_and_spawn_failure_finish_the_parent_run() {
        let cases = vec![
            ("parent-exit-0", 0, RunOutcome::Succeeded, None),
            ("parent-exit-23", 23, RunOutcome::Failed, None),
            ("parent-exit-130", 130, RunOutcome::Interrupted, None),
            ("missing", 1, RunOutcome::Failed, Some("failed to re-exec")),
        ];
        #[cfg(unix)]
        let cases = {
            let mut cases = cases;
            cases.push((
                "parent-signal",
                1,
                RunOutcome::Failed,
                Some("child process terminated by signal"),
            ));
            cases
        };
        for (role, code, expected_outcome, diagnostic) in cases {
            let fixture = tempfile::tempdir().unwrap();
            let result = ProcessExecutor::system()
                .execute(
                    CommandSpec::new(std::env::current_exe().unwrap())
                        .args(&["--ignored", "--exact", CHILD_TEST, "--nocapture"])
                        .env(CHILD_ROLE, role)
                        .env(CHILD_ROOT, fixture.path().as_os_str())
                        .timeout(std::time::Duration::from_secs(20))
                        .unchecked(),
                )
                .unwrap();
            assert_eq!(result.code, Some(code), "{role}: {result:?}");
            let path = std::fs::read_to_string(fixture.path().join("run-path")).unwrap();
            let contents = std::fs::read_to_string(path).unwrap();
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
            assert_eq!(finishes, [(expected_outcome, code)], "{role}: {contents}");
            if let Some(diagnostic) = diagnostic {
                assert!(contents.contains(diagnostic), "{role}: {contents}");
            }
        }
    }
}
