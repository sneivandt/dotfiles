//! Command execution abstractions.
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

/// Create a new [`Command`] with platform-appropriate defaults.
///
/// On Unix the child is placed in its own process group so that a
/// `SIGINT` from Ctrl-C reaches only the Rust process (via the
/// cooperative cancellation token) and does not kill child processes
/// that are still running.
fn new_command(program: &str) -> Command {
    #[allow(unused_mut, reason = "platform-specific mutability")]
    let mut cmd = Command::new(program);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    cmd
}

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
    log_command_output(label, &result);
    if !result.success {
        let code = result.code.unwrap_or(-1);
        bail!("{label} failed (exit {code}): {}", failure_output(&result));
    }
    Ok(result)
}

/// Log captured child-process output at debug level.
fn log_command_output(label: &str, result: &ExecResult) {
    log_stream(label, "stdout", &result.stdout, result.success);
    log_stream(label, "stderr", &result.stderr, result.success);
}

/// Log one captured output stream line-by-line.
fn log_stream(label: &str, stream: &str, output: &str, success: bool) {
    let summary = stream_summary(output);
    if summary.is_empty() {
        return;
    }

    if success {
        tracing::debug!(
            target: "dotfiles::exec",
            "{label} {stream}: {summary} suppressed on success"
        );
        return;
    }

    tracing::debug!(target: "dotfiles::exec", "{label} {stream}: {summary}");
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        tracing::debug!(target: "dotfiles::exec", "{label} {stream}: {line}");
    }
}

/// Summarise a captured child-process output stream.
fn stream_summary(output: &str) -> String {
    let line_count = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if line_count == 0 {
        return String::new();
    }

    let line_word = if line_count == 1 { "line" } else { "lines" };
    format!("{line_count} {line_word}, {} bytes", output.len())
}

/// Format stdout/stderr for a failed command error message.
fn failure_output(result: &ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => format!("stdout: {stdout}"),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout: {stdout}; stderr: {stderr}"),
    }
}

/// Trait for executing system commands, enabling test injection.
///
/// Implement this trait to provide mock executors for unit tests.
/// The [`SystemExecutor`] implementation delegates to real process spawning.
#[cfg_attr(test, mockall::automock)]
pub trait Executor: std::fmt::Debug + Send + Sync {
    /// Execute a command, bailing on non-zero exit.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, cannot be found,
    /// or exits with a non-zero status code.
    #[cfg_attr(test, mockall::concretize)]
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Execute a command in a specific directory.
    ///
    /// The default implementation delegates to
    /// [`run_in_with_env`](Executor::run_in_with_env) with an empty
    /// environment slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, the directory does not exist,
    /// or the command exits with a non-zero status code.
    #[cfg_attr(test, mockall::concretize)]
    fn run_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        self.run_in_with_env(dir, program, args, &[])
    }

    /// Execute a command in a specific directory with extra environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute, the directory does not exist,
    /// or the command exits with a non-zero status code.
    #[cfg_attr(test, mockall::concretize)]
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
    #[cfg_attr(test, mockall::concretize)]
    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Execute a command in a specific directory, allowing non-zero exit.
    ///
    /// The default implementation ignores `dir` and delegates to
    /// [`run_unchecked`](Executor::run_unchecked); concrete executors should
    /// override it to actually run the command in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute or cannot be found,
    /// but does NOT fail on non-zero exit codes (which are captured in the result).
    #[cfg_attr(test, mockall::concretize)]
    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        let _ = dir;
        self.run_unchecked(program, args)
    }

    /// Check if a program is available on PATH.
    #[cfg_attr(not(test), must_use)]
    fn which(&self, program: &str) -> bool;

    /// Resolve the full path of a program on PATH.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be found on PATH.
    fn which_path(&self, program: &str) -> Result<std::path::PathBuf>;
}

/// The real system executor that delegates to process spawning.
#[derive(Debug)]
pub struct SystemExecutor;

impl Executor for SystemExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        execute_checked(cmd, program)
    }

    fn run_in_with_env(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args).current_dir(dir);
        for (k, v) in env {
            cmd.env(k, v);
        }
        execute_checked(cmd, &format!("{program} in {}", dir.display()))
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let output = new_command(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to execute: {program}"))?;
        let result = ExecResult::from(output);
        log_command_output(program, &result);
        Ok(result)
    }

    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        let output = new_command(program)
            .args(args)
            .current_dir(dir)
            .output()
            .with_context(|| format!("failed to execute: {program} in {}", dir.display()))?;
        let result = ExecResult::from(output);
        log_command_output(&format!("{program} in {}", dir.display()), &result);
        Ok(result)
    }

    fn which(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }

    fn which_path(&self, program: &str) -> Result<std::path::PathBuf> {
        which::which(program).with_context(|| format!("{program} not found on PATH"))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    /// Helper: run a simple echo command cross-platform.
    fn echo_result(msg: &str) -> Result<ExecResult> {
        let executor = SystemExecutor;
        #[cfg(windows)]
        {
            executor.run("cmd", &["/C", "echo", msg])
        }
        #[cfg(not(windows))]
        {
            executor.run("echo", &[msg])
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
        let executor = SystemExecutor;
        #[cfg(windows)]
        let result = executor.run("cmd", &["/C", "exit", "1"]);
        #[cfg(not(windows))]
        let result = executor.run("false", &[]);
        assert!(result.is_err(), "non-zero exit should produce an error");
    }

    #[test]
    fn run_unchecked_failure() {
        let executor = SystemExecutor;
        #[cfg(windows)]
        let result = executor.run_unchecked("cmd", &["/C", "exit", "1"]).unwrap();
        #[cfg(not(windows))]
        let result = executor.run_unchecked("false", &[]).unwrap();
        assert!(!result.success, "non-zero exit should set success=false");
    }

    #[test]
    fn which_finds_known_program() {
        let executor = SystemExecutor;
        // `cmd` always exists on Windows; `echo` is a real binary on Unix.
        #[cfg(windows)]
        assert!(executor.which("cmd"), "cmd should be found on Windows");
        #[cfg(not(windows))]
        assert!(executor.which("echo"), "echo should be found on Unix");
    }

    #[test]
    fn which_missing_program() {
        let executor = SystemExecutor;
        assert!(
            !executor.which("this-program-does-not-exist-12345"),
            "non-existent program should not be found"
        );
    }

    #[test]
    fn which_path_finds_known_program() {
        let executor = SystemExecutor;
        #[cfg(windows)]
        let result = executor.which_path("cmd");
        #[cfg(not(windows))]
        let result = executor.which_path("echo");
        assert!(result.is_ok(), "which_path should find a known program");
        let path = result.unwrap();
        assert!(
            path.is_absolute(),
            "which_path should return an absolute path"
        );
    }

    #[test]
    fn which_path_fails_for_missing_program() {
        let executor = SystemExecutor;
        let result = executor.which_path("this-program-does-not-exist-12345");
        assert!(
            result.is_err(),
            "which_path should fail for a missing program"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found on PATH"),
            "error message should mention 'not found on PATH'"
        );
    }

    #[test]
    fn system_executor_which_path_finds_known_program() {
        let executor = SystemExecutor;
        #[cfg(windows)]
        let result = executor.which_path("cmd");
        #[cfg(not(windows))]
        let result = executor.which_path("echo");
        assert!(
            result.is_ok(),
            "SystemExecutor::which_path should find a known program"
        );
    }

    #[test]
    fn system_executor_which_path_fails_for_missing() {
        let executor = SystemExecutor;
        let result = executor.which_path("this-program-does-not-exist-12345");
        assert!(
            result.is_err(),
            "SystemExecutor::which_path should fail for missing program"
        );
    }

    #[test]
    fn run_in_tempdir() {
        let executor = SystemExecutor;
        let dir = std::env::temp_dir();
        #[cfg(windows)]
        let result = executor
            .run_in(&dir, "cmd", &["/C", "echo", "hello"])
            .unwrap();
        #[cfg(not(windows))]
        let result = executor.run_in(&dir, "echo", &["hello"]).unwrap();
        assert!(result.success, "echo in temp dir should succeed");
    }

    #[test]
    fn stream_summary_ignores_blank_output() {
        assert_eq!(stream_summary("\n \n"), "");
    }

    #[test]
    fn stream_summary_counts_non_empty_lines() {
        assert_eq!(stream_summary("one\n\n two \n"), "2 lines, 11 bytes");
    }
}
