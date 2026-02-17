use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

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

/// Check if a program is available on PATH.
#[must_use]
pub fn which(program: &str) -> bool {
    #[cfg(target_os = "windows")]
    let check = Command::new("where").arg(program).output();

    #[cfg(not(target_os = "windows"))]
    let check = Command::new("which").arg(program).output();

    check.is_ok_and(|o| o.status.success())
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
        assert!(result.success, "echo command should succeed");
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[test]
    fn run_failure() {
        #[cfg(windows)]
        let result = run("cmd", &["/C", "exit", "1"]);
        #[cfg(not(windows))]
        let result = run("false", &[]);
        assert!(result.is_err(), "non-zero exit should produce an error");
    }

    #[test]
    fn run_unchecked_failure() {
        #[cfg(windows)]
        let result = run_unchecked("cmd", &["/C", "exit", "1"]).unwrap();
        #[cfg(not(windows))]
        let result = run_unchecked("false", &[]).unwrap();
        assert!(!result.success, "non-zero exit should set success=false");
    }

    #[test]
    fn which_finds_known_program() {
        // `cmd` always exists on Windows; `echo` is a real binary on Unix.
        #[cfg(windows)]
        assert!(which("cmd"), "cmd should be found on Windows");
        #[cfg(not(windows))]
        assert!(which("echo"), "echo should be found on Unix");
    }

    #[test]
    fn which_missing_program() {
        assert!(
            !which("this-program-does-not-exist-12345"),
            "non-existent program should not be found"
        );
    }

    #[test]
    fn run_in_tempdir() {
        let dir = std::env::temp_dir();
        #[cfg(windows)]
        let result = run_in(&dir, "cmd", &["/C", "echo", "hello"]).unwrap();
        #[cfg(not(windows))]
        let result = run_in(&dir, "echo", &["hello"]).unwrap();
        assert!(result.success, "echo in temp dir should succeed");
    }
}
