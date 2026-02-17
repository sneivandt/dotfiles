use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

use crate::logging::Logger;

/// Result of a command execution.
#[derive(Debug)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub code: Option<i32>,
}

impl From<Output> for ExecResult {
    fn from(output: Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
            code: output.status.code(),
        }
    }
}

/// Execute a command and return the result, bailing on non-zero exit.
fn execute_checked(mut cmd: Command, label: &str) -> Result<ExecResult> {
    let output = cmd
        .output()
        .with_context(|| format!("failed to execute: {label}"))?;
    let result = ExecResult::from(output);
    if !result.success {
        bail!(
            "{label} failed (exit {}): {}",
            result.code.unwrap_or(-1),
            result.stderr.trim()
        );
    }
    Ok(result)
}

/// Run a command and return its output. Fails if the command exits non-zero.
pub fn run(program: &str, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    execute_checked(cmd, program)
}

/// Run a command in a specific directory.
pub fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir);
    execute_checked(cmd, &format!("{program} in {}", dir.display()))
}

/// Run a command in a specific directory with extra environment variables.
pub fn run_in_with_env(
    dir: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<ExecResult> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    execute_checked(cmd, &format!("{program} in {}", dir.display()))
}

/// Run a command, allowing failure (returns result without bailing).
pub fn run_unchecked(program: &str, args: &[&str]) -> Result<ExecResult> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute: {program}"))?;

    Ok(ExecResult::from(output))
}

/// Run a command with output inherited to the terminal (for interactive commands).
#[allow(dead_code)]
pub fn run_interactive(program: &str, args: &[&str]) -> Result<bool> {
    let status = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("failed to execute: {program}"))?;

    Ok(status.success())
}

/// Check if a program is available on PATH.
#[must_use]
pub fn which(program: &str) -> bool {
    #[cfg(target_os = "windows")]
    let check = Command::new("where").arg(program).output();

    #[cfg(not(target_os = "windows"))]
    let check = Command::new("which").arg(program).output();

    check.is_ok_and(|o| o.status.success())
}

/// Run a command with dry-run guard. If `dry_run` is true, logs the command
/// instead of executing it. Returns Ok(None) for dry-run, Ok(Some(result))
/// for real execution.
#[allow(dead_code)]
pub fn run_guarded(
    dry_run: bool,
    log: &Logger,
    program: &str,
    args: &[&str],
) -> Result<Option<ExecResult>> {
    if dry_run {
        let cmd = format!("{program} {}", args.join(" "));
        log.dry_run(&cmd);
        return Ok(None);
    }
    run(program, args).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: run a simple echo command cross-platform.
    fn echo_result(msg: &str) -> Result<ExecResult> {
        #[cfg(windows)]
        {
            run("cmd", &["/C", "echo", msg])
        }
        #[cfg(not(windows))]
        {
            run("echo", &[msg])
        }
    }

    #[test]
    fn run_echo() {
        let result = echo_result("hello").unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[test]
    fn run_failure() {
        #[cfg(windows)]
        let result = run("cmd", &["/C", "exit", "1"]);
        #[cfg(not(windows))]
        let result = run("false", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn run_unchecked_failure() {
        #[cfg(windows)]
        let result = run_unchecked("cmd", &["/C", "exit", "1"]).unwrap();
        #[cfg(not(windows))]
        let result = run_unchecked("false", &[]).unwrap();
        assert!(!result.success);
    }

    #[test]
    fn which_finds_known_program() {
        // `cmd` always exists on Windows; `echo` is a real binary on Unix.
        #[cfg(windows)]
        assert!(which("cmd"));
        #[cfg(not(windows))]
        assert!(which("echo"));
    }

    #[test]
    fn which_missing_program() {
        assert!(!which("this-program-does-not-exist-12345"));
    }

    #[test]
    fn run_guarded_dry_run() {
        let log = Logger::new(false);
        let result = run_guarded(true, &log, "echo", &["test"]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn run_guarded_real() {
        let log = Logger::new(false);
        #[cfg(windows)]
        let result = run_guarded(false, &log, "cmd", &["/C", "echo", "test"]).unwrap();
        #[cfg(not(windows))]
        let result = run_guarded(false, &log, "echo", &["test"]).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().success);
    }

    #[test]
    fn run_in_tempdir() {
        let dir = std::env::temp_dir();
        #[cfg(windows)]
        let result = run_in(&dir, "cmd", &["/C", "echo", "hello"]).unwrap();
        #[cfg(not(windows))]
        let result = run_in(&dir, "echo", &["hello"]).unwrap();
        assert!(result.success);
    }
}
