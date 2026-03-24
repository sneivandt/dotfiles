//! Windows privilege elevation.
//!
//! Detects whether the process has administrator privileges and re-launches
//! elevated via UAC when needed. No-op on non-Windows platforms.

/// Check if the current process is running with administrator privileges.
///
/// On Windows, runs `net session` which succeeds only when elevated.
/// On non-Windows platforms, always returns `false`.
#[cfg(windows)]
#[must_use]
pub fn is_elevated() -> bool {
    use std::process::{Command, Stdio};

    Command::new("net")
        .arg("session")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Check if the current process is running with administrator privileges.
///
/// Always returns `false` on non-Windows platforms.
#[cfg(not(windows))]
#[must_use]
pub const fn is_elevated() -> bool {
    false
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
#[allow(clippy::print_stderr)]
pub fn elevate_and_exit(executor: &dyn crate::exec::Executor) -> anyhow::Result<()> {
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

    eprintln!("Not running as administrator. Requesting elevation...");

    let status = std::process::Command::new(ps_exe)
        .args([
            "-NoProfile",
            "-EncodedCommand",
            &powershell_encode_command(&ps_script),
        ])
        .status()
        .context("failed to start elevated process")?;

    if status.success() {
        eprintln!("Elevated window opened.");
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
#[allow(clippy::print_stderr)]
pub fn wait_if_elevated() {
    if is_elevated() {
        eprintln!();
        eprint!("Press Enter to close...");
        std::io::stdin().read_line(&mut String::new()).ok(); // Best-effort: ignore read errors
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub const fn wait_if_elevated() {}

/// Wrap `value` in `PowerShell` single quotes, doubling any embedded single
/// quotes so the value is interpreted literally by `PowerShell`.
#[cfg(any(windows, test))]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Build a `PowerShell` `@(…)` array literal from a slice of argument strings.
///
/// Each element is individually single-quoted via [`powershell_single_quote`].
/// Returns `"@()"` for an empty slice.
#[cfg(any(windows, test))]
fn powershell_arg_list(args: &[String]) -> String {
    if args.is_empty() {
        "@()".to_string()
    } else {
        format!(
            "@({})",
            args.iter()
                .map(|a| powershell_single_quote(a))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Encode a `PowerShell` script string as Base64 UTF-16LE, suitable for the
/// `PowerShell` `-EncodedCommand` parameter.
#[cfg(any(windows, test))]
pub(crate) fn powershell_encode_command(script: &str) -> String {
    let utf16_le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64_encode(&utf16_le)
}

/// Encode `bytes` as standard Base64 (RFC 4648) without line wrapping.
#[cfg(any(windows, test))]
fn base64_encode(bytes: &[u8]) -> String {
    #[allow(clippy::cast_possible_truncation)] // n & 63 fits in u8
    fn encode_6bit(n: u32) -> char {
        match n & 63 {
            v @ 0..=25 => char::from(b'A' + v as u8),
            v @ 26..=51 => char::from(b'a' + v as u8 - 26),
            v @ 52..=61 => char::from(b'0' + v as u8 - 52),
            62 => '+',
            _ => '/',
        }
    }

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(encode_6bit(n >> 18));
        out.push(encode_6bit(n >> 12));
        out.push(if chunk.len() > 1 {
            encode_6bit(n >> 6)
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { encode_6bit(n) } else { '=' });
    }
    out
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

    // --- base64_encode ---

    #[test]
    fn base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn base64_encode_one_byte() {
        // 0x00 -> 000000 00|0000 -> "AA=="
        assert_eq!(base64_encode(&[0x00]), "AA==");
    }

    #[test]
    fn base64_encode_two_bytes() {
        // 0x00 0x00 -> "AAA="
        assert_eq!(base64_encode(&[0x00, 0x00]), "AAA=");
    }

    #[test]
    fn base64_encode_three_bytes() {
        // 0x00 0x00 0x00 -> "AAAA"
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn base64_encode_known_value() {
        // "Man" -> "TWFu" (standard RFC 4648 test vector)
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }
}
