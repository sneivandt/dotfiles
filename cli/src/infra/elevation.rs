//! Cross-platform privilege elevation mechanisms.
//!
//! Provides scoped Windows UAC delegation and Unix sudo credential-cache
//! support. Neither platform elevates the whole run: elevation is requested
//! only for the specific tasks that declare `needs_elevation`, and declining it
//! degrades those tasks instead of aborting the run.

#[cfg(windows)]
use crate::infra::exec::windows::PowerShellCommand;
#[cfg(test)]
use crate::infra::exec::windows::powershell_encode_command;
#[cfg(any(windows, test))]
use crate::infra::exec::windows::{powershell_argument_line, powershell_single_quote};
#[cfg(windows)]
use crate::infra::logging::OutputExt as _;

/// Environment variable marking a process as an elevated child of another run.
///
/// The child must never plan elevation of its own, otherwise a failed task
/// could spawn an unbounded chain of UAC prompts.
pub(crate) const ELEVATED_CHILD_VAR: &str = "DOTFILES_ELEVATED_CHILD";

/// Exit code the elevation helper reports when the UAC prompt is declined.
///
/// Matches the Win32 `ERROR_CANCELLED` value that `Start-Process -Verb RunAs`
/// surfaces when the user dismisses the consent dialog.
#[cfg(any(windows, test))]
pub(crate) const ELEVATION_DECLINED_EXIT_CODE: i32 = 1223;

/// Return whether this process was spawned as the elevated child of a parent run.
///
/// Resolved once: from [`mark_elevated_child`] when the CLI flag is present, or
/// from the environment as a fallback for wrappers that set it directly.
#[must_use]
#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "only the Windows exit wait uses process-wide child state"
    )
)]
pub(crate) fn is_elevated_child() -> bool {
    *ELEVATED_CHILD
        .get_or_init(|| env_marks_elevated_child(std::env::var_os(ELEVATED_CHILD_VAR).as_deref()))
}

fn env_marks_elevated_child(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// Record that this process is the elevated child of a parent run.
///
/// Must be called before any [`is_elevated_child`] query; later calls are
/// ignored so the answer stays stable for the lifetime of the process.
pub(crate) fn mark_elevated_child() {
    // `set` reports Err once the flag has already been resolved, which is the
    // documented no-op rather than a failure.
    let _already_resolved = ELEVATED_CHILD.set(true).is_err();
}

/// Cached answer for [`is_elevated_child`].
static ELEVATED_CHILD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Check if the current process is running with administrator privileges.
///
/// On Windows, runs `net session` which succeeds only when elevated. The answer
/// is cached because a process cannot change its own elevation level, and
/// `needs_elevation` predicates consult it once per task.
///
/// On non-Windows platforms, always returns `false`.
#[cfg(windows)]
#[must_use]
pub(crate) fn is_elevated_windows() -> bool {
    use crate::infra::exec::{CommandSpec, Executor as _};
    use std::sync::OnceLock;

    static ELEVATED: OnceLock<bool> = OnceLock::new();

    *ELEVATED.get_or_init(|| {
        crate::infra::exec::ProcessExecutor::system()
            .execute(CommandSpec::new("net").arg("session").unchecked())
            .is_ok_and(|result| result.success)
    })
}

#[cfg(windows)]
pub(crate) use is_elevated_windows as is_elevated;

/// Check if the current process is running with administrator privileges.
///
/// Always returns `false` on non-Windows platforms.
#[cfg(not(windows))]
#[must_use]
pub(crate) const fn is_elevated() -> bool {
    false
}

/// Return whether `sudo` is available through the configured executor.
#[cfg(unix)]
#[must_use]
pub(crate) fn sudo_available(executor: &dyn crate::infra::exec::Executor) -> bool {
    executor.which("sudo")
}

/// Return whether sudo credentials are already cached.
#[cfg(unix)]
#[must_use]
pub(crate) fn sudo_credentials_cached() -> bool {
    use std::process::Stdio;

    std::process::Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Prompt for sudo credentials through the controlling terminal.
///
/// # Errors
///
/// Returns an error if the `sudo` process cannot be started.
#[cfg(unix)]
pub(crate) fn prime_sudo_credentials() -> std::io::Result<bool> {
    use std::process::Stdio;

    let tty_in = std::fs::File::open("/dev/tty");
    let tty_out = std::fs::OpenOptions::new().write(true).open("/dev/tty");
    let mut command = std::process::Command::new("sudo");
    command.arg("-v");
    if let Ok(file) = tty_in {
        command.stdin(Stdio::from(file));
    }
    if let Ok(file) = tty_out {
        command.stderr(Stdio::from(file));
    }

    command.status().map(|status| status.success())
}

/// Build the `PowerShell` script that runs `exe` elevated and waits for it.
///
/// Kept as a pure string builder so the escaping and exit-code plumbing can be
/// asserted on any platform, following the pattern used by the re-exec helper.
///
/// The script maps a declined UAC prompt onto [`ELEVATION_DECLINED_EXIT_CODE`]
/// so the caller can distinguish "user said no" from "the child run failed".
#[cfg(any(windows, test))]
pub(crate) fn build_elevated_child_script(exe: &str, args: &[String], cwd: &str) -> String {
    let exe_quoted = powershell_single_quote(exe);
    let cwd_quoted = powershell_single_quote(cwd);
    let start = if args.is_empty() {
        format!(
            "Start-Process -FilePath {exe_quoted} -WorkingDirectory {cwd_quoted} -Verb RunAs -Wait -PassThru"
        )
    } else {
        let arg_list = powershell_argument_line(args);
        format!(
            "Start-Process -FilePath {exe_quoted} -ArgumentList {arg_list} -WorkingDirectory {cwd_quoted} -Verb RunAs -Wait -PassThru"
        )
    };

    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{ $p = {start} }} catch {{\n\
             $exception = $_.Exception\n\
             while ($null -ne $exception) {{\n\
                 if ($exception -is [System.ComponentModel.Win32Exception] -and \
                     $exception.NativeErrorCode -eq {ELEVATION_DECLINED_EXIT_CODE}) {{\n\
                     exit {ELEVATION_DECLINED_EXIT_CODE}\n\
                 }}\n\
                 $exception = $exception.InnerException\n\
             }}\n\
             throw\n\
         }}\n\
         if ($null -eq $p) {{ throw 'Elevated process did not return a process handle' }}\n\
         $exitCode = $p.ExitCode\n\
         if ($null -eq $exitCode) {{ throw 'Cannot determine elevated process exit code' }}\n\
         exit $exitCode\n"
    )
}

/// Result of delegating work to an elevated child process.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElevationOutcome {
    /// The elevated child ran to completion successfully.
    Completed,
    /// The user dismissed the UAC consent dialog.
    Declined,
    /// The elevated child ran but reported a failure.
    Failed(i32),
}

/// Run `args` in an elevated child process and wait for it to finish.
///
/// Uses `PowerShell` `Start-Process -Verb RunAs -Wait -PassThru` to trigger the
/// UAC prompt. The current process keeps running unelevated: only the child
/// holds the administrator token, and only for the tasks it was scoped to.
///
/// The `PowerShell` script is Base64-encoded as UTF-16LE and passed via
/// `-EncodedCommand` so the outer command string is never parsed by
/// `PowerShell`, eliminating any risk of argument injection from args that
/// contain special characters such as single quotes, newlines, or commas.
///
/// # Errors
///
/// Returns an error if the current executable cannot be located or the
/// `PowerShell` helper itself cannot be started.
#[cfg(windows)]
pub(crate) fn run_elevated_child(
    executor: &dyn crate::infra::exec::Executor,
    log: &dyn crate::infra::logging::Output,
    args: &[String],
) -> anyhow::Result<ElevationOutcome> {
    use anyhow::Context as _;

    let exe = std::env::current_exe().context("failed to determine current executable path")?;
    let cwd =
        std::env::current_dir().context("failed to determine elevated child working directory")?;
    let child_args = log.run_id().map_or_else(
        || args.to_vec(),
        |id| crate::infra::logging::records::child_args(args, &id),
    );
    let script = build_elevated_child_script(
        &exe.display().to_string(),
        &child_args,
        &cwd.display().to_string(),
    );

    let ps_exe = if executor.which("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };

    log.debug(format!("elevating: {} {}", exe.display(), args.join(" ")));

    let result = PowerShellCommand::new(&script)
        .run_unchecked(executor, ps_exe)
        .context("failed to start elevated process")?;

    Ok(match result.code {
        Some(0) => ElevationOutcome::Completed,
        Some(ELEVATION_DECLINED_EXIT_CODE) => ElevationOutcome::Declined,
        Some(code) => ElevationOutcome::Failed(code),
        None => ElevationOutcome::Failed(-1),
    })
}

/// Pause before exiting so the user can read output in an elevated window.
///
/// Only applies to a child this process tree spawned for elevation: that window
/// is created by `Start-Process` and closes as soon as the child exits, so
/// without the pause its output would be unreadable. A normal run — elevated or
/// not — owns the user's existing terminal and must never block on exit.
#[cfg(windows)]
#[expect(clippy::print_stderr, reason = "intentional user-facing output")]
pub(crate) fn wait_if_elevated(non_interactive: bool) {
    // Never block on input where nothing can supply it: a `--elevated-child`
    // run driven by automation would otherwise stall until the command timeout.
    if should_pause_on_exit(
        non_interactive,
        is_elevated_child(),
        std::env::var_os("CI").is_some(),
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
    ) {
        eprintln!();
        eprint!("Press Enter to close...");
        drop(std::io::stdin().read_line(&mut String::new())); // Best-effort: ignore read errors
    }
}

/// Pause only an interactive elevated window with an available input terminal.
#[cfg(any(windows, test))]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "independent execution and terminal capabilities"
)]
const fn should_pause_on_exit(
    non_interactive: bool,
    elevated_child: bool,
    ci: bool,
    terminal: bool,
) -> bool {
    !non_interactive && elevated_child && !ci && terminal
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub(crate) const fn wait_if_elevated(_non_interactive: bool) {}

#[cfg(test)]
#[path = "tests/elevation.rs"]
mod unit_tests;
