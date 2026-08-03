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
use crate::infra::exec::windows::{powershell_arg_list, powershell_single_quote};
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
pub fn is_elevated_child() -> bool {
    *ELEVATED_CHILD
        .get_or_init(|| std::env::var_os(ELEVATED_CHILD_VAR).is_some_and(|value| !value.is_empty()))
}

/// Record that this process is the elevated child of a parent run.
///
/// Must be called before any [`is_elevated_child`] query; later calls are
/// ignored so the answer stays stable for the lifetime of the process.
pub fn mark_elevated_child() {
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
pub fn is_elevated() -> bool {
    use crate::infra::exec::Executor as _;
    use std::sync::OnceLock;

    static ELEVATED: OnceLock<bool> = OnceLock::new();

    *ELEVATED.get_or_init(|| {
        crate::infra::exec::ProcessExecutor::system()
            .run_unchecked("net", &["session"])
            .is_ok_and(|result| result.success)
    })
}

/// Check if the current process is running with administrator privileges.
///
/// Always returns `false` on non-Windows platforms.
#[cfg(not(windows))]
#[must_use]
#[allow(
    dead_code,
    reason = "called only from Windows cfg-gated elevation path"
)]
pub const fn is_elevated() -> bool {
    false
}

/// Return whether `sudo` is available through the configured executor.
#[cfg(unix)]
#[must_use]
pub fn sudo_available(executor: &dyn crate::infra::exec::Executor) -> bool {
    executor.which("sudo")
}

/// Return whether sudo credentials are already cached.
#[cfg(unix)]
#[must_use]
pub fn sudo_credentials_cached() -> bool {
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
pub fn prime_sudo_credentials() -> std::io::Result<bool> {
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
pub(crate) fn build_elevated_child_script(exe: &str, args: &[String]) -> String {
    let exe_quoted = powershell_single_quote(exe);
    let start = if args.is_empty() {
        format!("Start-Process -FilePath {exe_quoted} -Verb RunAs -Wait -PassThru")
    } else {
        let arg_list = powershell_arg_list(args);
        format!(
            "Start-Process -FilePath {exe_quoted} -ArgumentList {arg_list} -Verb RunAs -Wait -PassThru"
        )
    };

    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{ $p = {start} }} catch {{ exit {ELEVATION_DECLINED_EXIT_CODE} }}\n\
         if ($null -eq $p) {{ exit {ELEVATION_DECLINED_EXIT_CODE} }}\n\
         exit $p.ExitCode\n"
    )
}

/// Result of delegating work to an elevated child process.
#[cfg(windows)]
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
    let script = build_elevated_child_script(&exe.display().to_string(), args);

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
#[allow(clippy::print_stderr, reason = "intentional user-facing output")]
pub fn wait_if_elevated() {
    // Never block on input where nothing can supply it: a `--elevated-child`
    // run driven by automation would otherwise stall until the command timeout.
    if is_elevated_child()
        && std::env::var_os("CI").is_none()
        && std::io::IsTerminal::is_terminal(&std::io::stdin())
    {
        eprintln!();
        eprint!("Press Enter to close...");
        drop(std::io::stdin().read_line(&mut String::new())); // Best-effort: ignore read errors
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub const fn wait_if_elevated() {}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
    use super::*;

    #[test]
    fn is_elevated_returns_false_on_non_windows() {
        // On Linux/macOS, is_elevated() is a const fn that always returns false.
        assert!(!is_elevated());
    }

    #[test]
    fn wait_if_elevated_is_noop_on_non_windows() {
        // Should complete without blocking or panicking.
        wait_if_elevated();
    }

    #[test]
    fn elevated_child_flag_is_absent_by_default() {
        assert!(
            !is_elevated_child(),
            "a normal test process must not look like an elevated child"
        );
    }

    #[test]
    #[cfg(unix)]
    fn sudo_available_reflects_the_executor_lookup() {
        use crate::infra::exec::MockExecutor;

        let mut present = MockExecutor::new();
        present
            .expect_which()
            .withf(|program| program == "sudo")
            .returning(|_| true);
        assert!(sudo_available(&present));

        let mut absent = MockExecutor::new();
        absent
            .expect_which()
            .withf(|program| program == "sudo")
            .returning(|_| false);
        assert!(
            !sudo_available(&absent),
            "sudo must be reported unavailable when it is not on PATH"
        );
    }
}

#[cfg(test)]
mod escaping_tests {
    use super::*;

    // --- build_elevated_child_script ---

    #[test]
    fn elevated_child_script_waits_and_forwards_the_exit_code() {
        let script = build_elevated_child_script(
            r"C:\dotfiles\dotfiles.exe",
            &["install".to_string(), "--only".to_string()],
        );

        assert!(
            script.contains("-Verb RunAs -Wait -PassThru"),
            "the parent must block on the elevated child: {script}"
        );
        assert!(
            script.contains("exit $p.ExitCode"),
            "the child exit code must reach the parent: {script}"
        );
        assert!(
            script.contains(r"-FilePath 'C:\dotfiles\dotfiles.exe'"),
            "the executable path must be single-quoted: {script}"
        );
        assert!(
            script.contains("-ArgumentList @('install', '--only')"),
            "arguments must be passed as a quoted PowerShell list: {script}"
        );
    }

    #[test]
    fn elevated_child_script_maps_a_declined_prompt_to_a_distinct_code() {
        let script = build_elevated_child_script("dotfiles.exe", &[]);

        assert_eq!(
            script
                .matches(&ELEVATION_DECLINED_EXIT_CODE.to_string())
                .count(),
            2,
            "both the catch block and the null guard must report a decline: {script}"
        );
    }

    #[test]
    fn elevated_child_script_omits_the_argument_list_when_there_are_no_arguments() {
        let script = build_elevated_child_script("dotfiles.exe", &[]);

        assert!(
            !script.contains("-ArgumentList"),
            "an empty argument list must not be emitted: {script}"
        );
    }

    #[test]
    fn elevated_child_script_escapes_quotes_in_arguments() {
        let script = build_elevated_child_script("dotfiles.exe", &["O'Brien".to_string()]);

        assert!(
            script.contains("'O''Brien'"),
            "single quotes must be doubled inside the argument list: {script}"
        );
    }

    // --- powershell_single_quote ---

    #[test]
    fn single_quote_wraps_plain_string() {
        assert_eq!(powershell_single_quote("hello"), "'hello'");
    }

    #[test]
    fn single_quote_preserves_spaces() {
        assert_eq!(
            powershell_single_quote("path with spaces"),
            "'path with spaces'"
        );
    }

    #[test]
    fn single_quote_escapes_single_quote() {
        assert_eq!(powershell_single_quote("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn single_quote_escapes_multiple_single_quotes() {
        assert_eq!(powershell_single_quote("a''b"), "'a''''b'");
    }

    #[test]
    fn single_quote_preserves_newline() {
        // Literal newlines are valid inside PS single-quoted strings; the
        // encoding layer (Base64) makes them safe at the command level.
        assert_eq!(powershell_single_quote("foo\nbar"), "'foo\nbar'");
    }

    #[test]
    fn single_quote_preserves_carriage_return_lf() {
        assert_eq!(powershell_single_quote("foo\r\nbar"), "'foo\r\nbar'");
    }

    // --- powershell_arg_list ---

    #[test]
    fn arg_list_empty_produces_empty_array() {
        let args: Vec<String> = vec![];
        assert_eq!(powershell_arg_list(&args), "@()");
    }

    #[test]
    fn arg_list_single_arg() {
        let args = vec!["install".to_string()];
        assert_eq!(powershell_arg_list(&args), "@('install')");
    }

    #[test]
    fn arg_list_multiple_args_with_spaces() {
        let args = vec![
            "--root".to_string(),
            "C:\\My Documents\\dotfiles".to_string(),
        ];
        assert_eq!(
            powershell_arg_list(&args),
            "@('--root', 'C:\\My Documents\\dotfiles')"
        );
    }

    #[test]
    fn arg_list_handles_commas_inside_args() {
        // Commas inside args must not become array separators.
        let args = vec!["a,b".to_string(), "c,d".to_string()];
        assert_eq!(powershell_arg_list(&args), "@('a,b', 'c,d')");
    }

    #[test]
    fn arg_list_handles_single_quotes_inside_args() {
        let args = vec!["O'Brien".to_string(), "it's fine".to_string()];
        assert_eq!(powershell_arg_list(&args), "@('O''Brien', 'it''s fine')");
    }

    #[test]
    fn arg_list_combines_spaces_and_single_quotes() {
        // Covers the combination of path-with-spaces and name-with-single-quote
        // in the same call — both quoting rules must apply simultaneously.
        let args = vec![
            "C:\\Temp\\Path With Space".to_string(),
            "O'Brien".to_string(),
        ];
        assert_eq!(
            powershell_arg_list(&args),
            "@('C:\\Temp\\Path With Space', 'O''Brien')"
        );
    }

    #[test]
    fn arg_list_handles_newline_inside_arg() {
        let args = vec!["foo\nbar".to_string()];
        assert_eq!(powershell_arg_list(&args), "@('foo\nbar')");
    }

    #[test]
    fn arg_list_handles_carriage_return_inside_arg() {
        let args = vec!["foo\r\nbar".to_string()];
        assert_eq!(powershell_arg_list(&args), "@('foo\r\nbar')");
    }

    // --- powershell_encode_command ---

    #[test]
    fn encode_command_empty_string_produces_empty() {
        assert_eq!(powershell_encode_command(""), "");
    }

    #[test]
    fn encode_command_produces_utf16le_base64() {
        // "abc" in UTF-16LE is 61 00 62 00 63 00.
        // base64("61 00 62 00 63 00") == "YQBiAGMA"  (verified externally).
        assert_eq!(powershell_encode_command("abc"), "YQBiAGMA");
    }

    #[test]
    fn encode_command_output_contains_only_base64_chars() {
        let script =
            "Start-Process -FilePath 'C:\\foo\\bar' -ArgumentList @('install') -Verb RunAs";
        let encoded = powershell_encode_command(script);
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        );
    }
}
