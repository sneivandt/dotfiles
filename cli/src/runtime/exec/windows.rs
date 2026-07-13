//! Typed, injection-safe Windows shell command construction.
//!
//! Keep the small number of unavoidable `cmd.exe` and encoded `PowerShell`
//! launches behind one boundary so call sites cannot accidentally pass
//! metacharacter-bearing values as unquoted command text.

use anyhow::{Result, bail};
use base64::Engine as _;

use super::{ExecResult, Executor};

/// A `cmd.exe` command whose arguments are treated as quoted literals.
#[derive(Debug, Clone)]
pub(crate) struct CmdCommand {
    program: String,
    args: Vec<String>,
}

impl CmdCommand {
    /// Create a command for a `cmd.exe` builtin or script wrapper.
    pub(crate) fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Append one literal argument.
    #[must_use]
    pub(crate) fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append multiple literal arguments.
    #[must_use]
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "used by Windows-only command wrappers")
    )]
    pub(crate) fn args(mut self, args: &[&str]) -> Self {
        self.args.extend(args.iter().map(|arg| (*arg).to_string()));
        self
    }

    /// Execute the command without interpreting a non-zero status as an error.
    ///
    /// # Errors
    ///
    /// Returns an error when a literal cannot be represented safely in a
    /// `cmd.exe` command string or the process cannot be executed.
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "used by Windows-only command wrappers")
    )]
    pub(crate) fn run_unchecked(&self, executor: &dyn Executor) -> Result<ExecResult> {
        let command_line = self.command_line()?;
        executor.run_windows_cmd_unchecked(&command_line)
    }

    fn command_line(&self) -> Result<String> {
        let mut tokens = Vec::with_capacity(self.args.len().saturating_add(1));
        tokens.push(quote_cmd_literal(&self.program)?);
        for arg in &self.args {
            tokens.push(quote_cmd_literal(arg)?);
        }

        // `/S /C` expects an outer quote pair around a command whose executable
        // is itself quoted.
        Ok(format!("\"{}\"", tokens.join(" ")))
    }
}

fn quote_cmd_literal(value: &str) -> Result<String> {
    if value.contains(['\0', '\r', '\n', '"', '%']) {
        bail!("value cannot be represented safely in a cmd.exe command: {value:?}");
    }
    Ok(format!("\"{value}\""))
}

/// A `PowerShell` script encoded for the injection-safe `-EncodedCommand`
/// process boundary.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by Windows-only process launchers")
)]
pub(crate) struct PowerShellCommand {
    encoded: String,
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by Windows-only process launchers")
)]
impl PowerShellCommand {
    /// Encode a script as Base64 UTF-16LE.
    pub(crate) fn new(script: &str) -> Self {
        Self {
            encoded: powershell_encode_command(script),
        }
    }

    /// Execute the encoded script without interpreting a non-zero status as an
    /// error.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected `PowerShell` executable cannot run.
    pub(crate) fn run_unchecked(
        &self,
        executor: &dyn Executor,
        powershell: &str,
    ) -> Result<ExecResult> {
        executor.run_unchecked(powershell, &self.args())
    }

    /// Configure a standard process command to run this encoded script.
    pub(crate) fn configure(&self, command: &mut std::process::Command) {
        command.args(self.args());
    }

    fn args(&self) -> [&str; 3] {
        ["-NoProfile", "-EncodedCommand", &self.encoded]
    }
}

/// Wrap a value in `PowerShell` single quotes, doubling embedded quotes.
pub(crate) fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Build a `PowerShell` array literal from individually quoted arguments.
pub(crate) fn powershell_arg_list(args: &[String]) -> String {
    if args.is_empty() {
        "@()".to_string()
    } else {
        format!(
            "@({})",
            args.iter()
                .map(|arg| powershell_single_quote(arg))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Encode a `PowerShell` script string as Base64 UTF-16LE.
pub(crate) fn powershell_encode_command(script: &str) -> String {
    let utf16_le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16_le)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn cmd_command_quotes_metacharacters_as_literals() {
        let command = CmdCommand::new("mklink")
            .arg("/J")
            .arg(r"C:\Users\A&B\link")
            .arg(r"C:\repo\(managed)\target");

        assert_eq!(
            command.command_line().unwrap(),
            r#"""mklink" "/J" "C:\Users\A&B\link" "C:\repo\(managed)\target"""#
        );
    }

    #[test]
    fn cmd_command_rejects_expansion_and_quote_characters() {
        for unsafe_value in [r"%PATH%", "quoted\"value", "line\nbreak"] {
            let error = CmdCommand::new("echo")
                .arg(unsafe_value)
                .command_line()
                .unwrap_err();
            assert!(
                error.to_string().contains("cannot be represented safely"),
                "unexpected cmd literal error: {error}"
            );
        }
    }

    #[test]
    fn encode_command_produces_utf16le_base64() {
        assert_eq!(powershell_encode_command("abc"), "YQBiAGMA");
    }
}
