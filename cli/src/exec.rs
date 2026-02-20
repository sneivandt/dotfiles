use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

/// Result of a command execution.
#[derive(Debug)]
pub struct ExecResult {
    /// Standard output as UTF-8 string.
    pub stdout: String,
    /// Standard error as UTF-8 string.
    pub stderr: String,
    /// Whether the command exited successfully (status code 0).
    pub success: bool,
    /// Exit code if available, or None if terminated by signal.
    pub code: Option<i32>,
}

impl From<Output> for ExecResult {
    fn from(output: Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
        let code = result.code.unwrap_or(-1);
        bail!("{label} failed (exit {code}): {}", result.stderr.trim());
    }
    Ok(result)
}

/// Run a command and return its output. Fails if the command exits non-zero.
///
/// # Errors
///
/// Returns an error if the command cannot be executed or exits with non-zero status.
pub fn run(program: &str, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    execute_checked(cmd, program)
}

/// Run a command in a specific directory.
///
/// # Errors
///
/// Returns an error if the command cannot be executed or exits with non-zero status.
pub fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir);
    execute_checked(cmd, &format!("{program} in {}", dir.display()))
}

/// Run a command in a specific directory with extra environment variables.
///
/// # Errors
///
/// Returns an error if the command cannot be executed or exits with non-zero status.
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
///
/// # Errors
///
/// Returns an error only if the command cannot be executed (not for non-zero exit).
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
    which::which(program).is_ok()
}

/// Resolve the full path of a program on PATH.
///
/// # Errors
///
/// Returns an error if the program cannot be found on PATH.
pub fn which_path(program: &str) -> Result<std::path::PathBuf> {
    which::which(program).with_context(|| format!("{program} not found on PATH"))
}

/// Trait for executing system commands, enabling test injection.
///
/// Implement this trait to provide mock executors for unit tests.
/// The [`SystemExecutor`] implementation delegates to the real free functions.
#[allow(dead_code)] // run_in and run_in_with_env complete the trait contract
pub trait Executor: std::fmt::Debug + Sync {
    /// Execute a command, bailing on non-zero exit.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, cannot be found,
    /// or exits with a non-zero status code.
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Execute a command in a specific directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, the directory does not exist,
    /// or the command exits with a non-zero status code.
    fn run_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Execute a command in a specific directory with extra environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, the directory does not exist,
    /// or the command exits with a non-zero status code.
    fn run_in_with_env(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<ExecResult>;

    /// Execute a command, allowing non-zero exit.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute or cannot be found,
    /// but does NOT fail on non-zero exit codes (which are captured in the result).
    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Check if a program is available on PATH.
    fn which(&self, program: &str) -> bool;
}

/// The real system executor that delegates to process spawning.
#[derive(Debug)]
pub struct SystemExecutor;

impl Executor for SystemExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        run(program, args)
    }

    fn run_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        run_in(dir, program, args)
    }

    fn run_in_with_env(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<ExecResult> {
        run_in_with_env(dir, program, args, env)
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        run_unchecked(program, args)
    }

    fn which(&self, program: &str) -> bool {
        which(program)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
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
