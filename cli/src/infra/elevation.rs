//! Cross-platform privilege elevation mechanisms.
//!
//! Provides Windows UAC re-launching and Unix sudo credential-cache support.

#[cfg(windows)]
use crate::infra::exec::windows::PowerShellCommand;
#[cfg(test)]
use crate::infra::exec::windows::powershell_encode_command;
#[cfg(any(windows, test))]
use crate::infra::exec::windows::{powershell_arg_list, powershell_single_quote};

/// Check if the current process is running with administrator privileges.
///
/// On Windows, runs `net session` which succeeds only when elevated.
/// On non-Windows platforms, always returns `false`.
#[cfg(windows)]
#[must_use]
pub fn is_elevated() -> bool {
    use crate::infra::exec::Executor as _;

    crate::infra::exec::SystemExecutor
        .run_unchecked("net", &["session"])
        .is_ok_and(|result| result.success)
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

/// Re-launch the current process with administrator privileges via UAC.
///
/// Uses `PowerShell` `Start-Process -Verb RunAs` to trigger the UAC prompt.
/// On success, an elevated window opens and the current process exits.
///
/// The `PowerShell` script is Base64-encoded as UTF-16LE and passed via
/// `-EncodedCommand` so the outer command string is never parsed by
/// `PowerShell`, eliminating any risk of argument injection from args that
/// contain special characters such as single quotes, newlines, or commas.
///
/// # Errors
///
/// Returns an error if the user cancels the UAC prompt or the elevated
/// process fails to start.
#[cfg(windows)]
pub fn elevate_and_exit(
    executor: &dyn crate::infra::exec::Executor,
    log: &dyn crate::infra::logging::Output,
) -> anyhow::Result<()> {
    use anyhow::{Context, bail};

    let exe = std::env::current_exe().context("failed to determine current executable path")?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let exe_quoted = powershell_single_quote(&exe.display().to_string());
    let ps_script = if args.is_empty() {
        format!("Start-Process -FilePath {exe_quoted} -Verb RunAs")
    } else {
        let arg_list = powershell_arg_list(&args);
        format!("Start-Process -FilePath {exe_quoted} -ArgumentList {arg_list} -Verb RunAs")
    };

    let ps_exe = if executor.which("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };

    log.always("Not running as administrator. Requesting elevation...");

    let result = PowerShellCommand::new(&ps_script)
        .run_unchecked(executor, ps_exe)
        .context("failed to start elevated process")?;

    if result.success {
        log.always("Elevated window opened.");
        std::process::exit(0);
    }

    bail!(
        "UAC elevation was cancelled or failed. \
         Administrator privileges are required. Use --dry-run to preview changes."
    );
}

/// Pause before exiting so the user can read output in an elevated window.
///
/// On Windows, if the process is elevated, prints a prompt and waits for
/// the user to press Enter. No-op on non-Windows or non-elevated processes.
#[cfg(windows)]
#[allow(clippy::print_stderr, reason = "intentional user-facing output")]
pub fn wait_if_elevated() {
    if is_elevated() {
        eprintln!();
        eprint!("Press Enter to close...");
        drop(std::io::stdin().read_line(&mut String::new())); // Best-effort: ignore read errors
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub const fn wait_if_elevated() {}

/// Detect which `PowerShell` executable is available on the current system.
///
/// Prefers `pwsh` (`PowerShell` 7+) when it is installed and functional;
/// falls back to `powershell` (Windows `PowerShell` 5.1) otherwise.
#[cfg(windows)]
pub(crate) fn preferred_powershell() -> &'static str {
    use crate::infra::exec::Executor as _;

    if crate::infra::exec::SystemExecutor
        .run_unchecked("pwsh", &["-NoProfile", "-Command", "exit 0"])
        .is_ok_and(|result| result.success)
    {
        "pwsh"
    } else {
        "powershell"
    }
}

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
}

#[cfg(test)]
mod escaping_tests {
    use super::*;

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
