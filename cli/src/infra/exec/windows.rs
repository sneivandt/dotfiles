//! Typed, injection-safe Windows shell command construction.
//!
//! Keep the small number of unavoidable `cmd.exe` and encoded `PowerShell`
//! launches behind one boundary so call sites cannot accidentally pass
//! metacharacter-bearing values as unquoted command text.

use anyhow::{Result, bail};
use base64::Engine as _;

use super::{CommandSpec, ExecError, ExecResult, Executor};

/// A `cmd.exe` command whose arguments are treated as literals.
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
        Ok(executor.execute(CommandSpec::windows_cmd(command_line))?)
    }

    fn command_line(&self) -> Result<String> {
        let mut tokens = Vec::with_capacity(self.args.len().saturating_add(1));
        tokens.push(quote_cmd_literal(&self.program)?);
        for arg in &self.args {
            tokens.push(quote_cmd_literal(arg)?);
        }

        // `/S /C` removes this outer quote pair, preserving quotes around paths
        // and arguments.
        Ok(format!("\"{}\"", tokens.join(" ")))
    }
}

fn quote_cmd_literal(value: &str) -> Result<String> {
    if value.contains(['\0', '\r', '\n', '"', '%']) {
        bail!("value cannot be represented safely in a cmd.exe command: {value:?}");
    }
    // Quoting builtin names (such as "mklink") prevents cmd from recognizing
    // them. Safe tokens, including builtin switches, need no quotes.
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./\\:-".contains(&byte))
    {
        return Ok(value.to_string());
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
    ) -> std::result::Result<ExecResult, ExecError> {
        executor.execute(
            CommandSpec::new(powershell)
                .args(&self.args())
                .redact_arguments()
                .unchecked(),
        )
    }

    fn args(&self) -> [&str; 3] {
        ["-NoProfile", "-EncodedCommand", &self.encoded]
    }
}

/// Wrap a value in `PowerShell` single quotes, doubling embedded quotes.
pub(crate) fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Build one Windows command-line string for `Start-Process -ArgumentList`.
///
/// `Start-Process` joins array elements before launching the child, which loses
/// argument boundaries for values containing spaces. Passing one pre-quoted
/// command line preserves the boundaries expected by the Windows argv parser.
pub(crate) fn powershell_argument_line(args: &[String]) -> String {
    let command_line = args
        .iter()
        .map(|arg| quote_windows_argument(arg))
        .collect::<Vec<_>>()
        .join(" ");
    powershell_single_quote(&command_line)
}

fn quote_windows_argument(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if !value
        .chars()
        .any(|character| character.is_ascii_whitespace() || matches!(character, '"' | '\\'))
    {
        return value.to_string();
    }

    let mut quoted = String::with_capacity(value.len().saturating_add(2));
    quoted.push('"');
    let mut backslashes = 0_usize;
    for character in value.chars() {
        match character {
            '\\' => backslashes = backslashes.saturating_add(1),
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes.saturating_mul(2)));
                quoted.push('\\');
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes.saturating_mul(2)));
    quoted.push('"');
    quoted
}

/// Encode a `PowerShell` script string as Base64 UTF-16LE.
pub(crate) fn powershell_encode_command(script: &str) -> String {
    let utf16_le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16_le)
}

#[cfg(test)]
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
            r#""mklink /J "C:\Users\A&B\link" "C:\repo\(managed)\target"""#
        );
    }

    #[test]
    fn cmd_command_keeps_builtin_tokens_unquoted_and_wrapper_paths_literal() {
        assert_eq!(
            CmdCommand::new("mklink")
                .arg("/J")
                .arg(r"C:\link")
                .arg(r"C:\source")
                .command_line()
                .unwrap(),
            r#""mklink /J C:\link C:\source""#
        );
        assert_eq!(
            CmdCommand::new(r"C:\Program Files\A&B\code.cmd")
                .arg("--install-extension")
                .arg("publisher.extension")
                .command_line()
                .unwrap(),
            r#"""C:\Program Files\A&B\code.cmd" --install-extension publisher.extension""#
        );
    }

    #[cfg(windows)]
    #[test]
    fn cmd_executes_a_script_wrapper_with_spaces_and_metacharacters() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let script = std::path::absolute(fixture.path().join("wrapper & (fixture).cmd")).unwrap();
        std::fs::write(&script, "@echo off\r\nexit /b 23\r\n").unwrap();

        let result = CmdCommand::new(script.to_str().unwrap())
            .run_unchecked(&crate::infra::exec::ProcessExecutor::system())
            .unwrap();

        assert_eq!(result.code, Some(23), "{result:?}");
    }

    #[test]
    fn cmd_command_rejects_expansion_and_quote_characters() {
        for unsafe_value in [
            r"%PATH%",
            "quoted\"value",
            "line\nbreak",
            "line\rbreak",
            "nul\0byte",
        ] {
            for command in [
                CmdCommand::new("echo").arg(unsafe_value),
                CmdCommand::new(unsafe_value).arg("safe"),
            ] {
                let executor = super::super::MockExecutor::new();
                let error = command.run_unchecked(&executor).unwrap_err();
                assert!(
                    error.to_string().contains("cannot be represented safely"),
                    "unexpected cmd literal error: {error}"
                );
            }
        }
    }

    #[test]
    fn encode_command_produces_utf16le_base64() {
        assert_eq!(powershell_encode_command("abc"), "YQBiAGMA");
    }

    #[test]
    fn powershell_encoding_preserves_non_ascii_and_surrogate_pairs() {
        let script = "Write-Output '日本語 🦀'\r\nWrite-Output \"O'Brien\"";
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(powershell_encode_command(script))
            .unwrap();
        assert_eq!(bytes.len() % 2, 0);
        let code_units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
            .collect();
        assert_eq!(String::from_utf16(&code_units).unwrap(), script);
    }

    #[test]
    fn windows_argument_quoting_preserves_empty_quotes_and_trailing_backslashes() {
        for (input, expected) in [
            ("", r#""""#),
            ("plain", "plain"),
            ("a\"b", r#""a\"b""#),
            ("path with space\\", r#""path with space\\""#),
            ("a\\\"b", r#""a\\\"b""#),
            ("a\tb", "\"a\tb\""),
        ] {
            assert_eq!(quote_windows_argument(input), expected, "{input:?}");
        }
    }
}
