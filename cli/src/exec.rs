//! Command execution abstractions.
use anyhow::{Context, Result, anyhow, bail};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::CancellationToken;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_mins(30);
#[cfg(not(windows))]
const SMOKE_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const TERMINATION_GRACE: Duration = Duration::from_secs(2);

/// Create a new [`Command`] with platform-appropriate defaults.
///
/// On Unix the child is placed in its own process group so that a
/// `SIGINT` from Ctrl-C reaches only the Rust process (via the
/// cooperative cancellation token).  The executor can then terminate the
/// whole child process group on cancellation or timeout.
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

/// Create a new [`Command`] from a path with platform-appropriate defaults.
#[cfg(not(windows))]
fn new_command_path(program: &Path) -> Command {
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

#[derive(Debug, Clone)]
struct CommandSettings {
    timeout: Duration,
    cancellation: Option<CancellationToken>,
}

impl CommandSettings {
    #[cfg(any(windows, test, feature = "internal-api"))]
    const fn default_timeout() -> Self {
        Self {
            timeout: DEFAULT_COMMAND_TIMEOUT,
            cancellation: None,
        }
    }

    #[cfg(not(windows))]
    const fn timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            cancellation: None,
        }
    }

    const fn managed(cancellation: CancellationToken, timeout: Duration) -> Self {
        Self {
            timeout,
            cancellation: Some(cancellation),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }
}

/// Execute a command and return the result, bailing on non-zero exit.
fn execute_checked(cmd: Command, label: &str, settings: &CommandSettings) -> Result<ExecResult> {
    let result = execute_unchecked(cmd, label, settings)?;
    log_command_output(label, &result);
    if !result.success {
        let code = result.code.unwrap_or(-1);
        bail!("{label} failed (exit {code}): {}", failure_output(&result));
    }
    Ok(result)
}

fn execute_unchecked(
    mut cmd: Command,
    label: &str,
    settings: &CommandSettings,
) -> Result<ExecResult> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to execute: {label}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture stdout for {label}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture stderr for {label}"))?;
    let stdout_reader = spawn_reader(stdout, "stdout", label);
    let stderr_reader = spawn_reader(stderr, "stderr", label);

    let start = Instant::now();
    let status = loop {
        if settings.is_cancelled() {
            #[cfg(unix)]
            terminate_child(&child);
            #[cfg(windows)]
            terminate_child(&mut child);
            wait_after_terminate(&mut child);
            let result = collect_result(None, stdout_reader, stderr_reader)?;
            log_command_output(label, &result);
            bail!("{label} cancelled: {}", failure_output(&result));
        }
        if start.elapsed() >= settings.timeout {
            #[cfg(unix)]
            terminate_child(&child);
            #[cfg(windows)]
            terminate_child(&mut child);
            wait_after_terminate(&mut child);
            let result = collect_result(None, stdout_reader, stderr_reader)?;
            log_command_output(label, &result);
            bail!(
                "{label} timed out after {} seconds: {}",
                settings.timeout.as_secs(),
                failure_output(&result)
            );
        }

        match child
            .try_wait()
            .with_context(|| format!("waiting for: {label}"))?
        {
            Some(status) => break status,
            None => std::thread::sleep(POLL_INTERVAL),
        }
    };

    collect_result(Some(status), stdout_reader, stderr_reader)
}

fn spawn_reader<R: Read + Send + 'static>(
    mut stream: R,
    name: &'static str,
    label: &str,
) -> JoinHandle<Result<Vec<u8>>> {
    let label = label.to_string();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        stream
            .read_to_end(&mut output)
            .with_context(|| format!("reading {name} from {label}"))?;
        Ok(output)
    })
}

fn collect_result(
    status: Option<std::process::ExitStatus>,
    stdout_reader: JoinHandle<Result<Vec<u8>>>,
    stderr_reader: JoinHandle<Result<Vec<u8>>>,
) -> Result<ExecResult> {
    let stdout = join_reader(stdout_reader, "stdout")?;
    let stderr = join_reader(stderr_reader, "stderr")?;
    let success = status.is_some_and(|s| s.success());
    let code = status.and_then(|s| s.code());
    Ok(ExecResult {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        success,
        code,
    })
}

fn join_reader(handle: JoinHandle<Result<Vec<u8>>>, name: &'static str) -> Result<Vec<u8>> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => bail!("{name} reader thread panicked"),
    }
}

#[cfg(unix)]
fn terminate_child(child: &Child) {
    terminate_process_tree(child);
}

#[cfg(windows)]
fn terminate_child(child: &mut Child) {
    terminate_process_tree(child);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to kill child: {err}");
    }
}

fn wait_after_terminate(child: &mut Child) {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                force_kill_child(child);
                return;
            }
            Ok(None) if started.elapsed() >= TERMINATION_GRACE => break,
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(err) => {
                tracing::debug!(target: "dotfiles::exec", "failed waiting after terminate: {err}");
                return;
            }
        }
    }
    force_kill_child(child);
    if let Err(err) = child.wait() {
        tracing::debug!(target: "dotfiles::exec", "failed waiting after force kill: {err}");
    }
}

#[cfg(unix)]
fn terminate_process_tree(child: &Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(windows)]
fn terminate_process_tree(child: &Child) {
    let pid = child.id().to_string();
    match Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::debug!(target: "dotfiles::exec", "taskkill failed with status {status}");
        }
        Err(err) => {
            tracing::debug!(target: "dotfiles::exec", "failed to run taskkill: {err}");
        }
    }
}

#[cfg(unix)]
fn force_kill_child(child: &mut Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to force-kill child: {err}");
    }
}

#[cfg(windows)]
fn force_kill_child(child: &mut Child) {
    terminate_process_tree(child);
    if let Err(err) = child.kill()
        && err.kind() != std::io::ErrorKind::InvalidInput
    {
        tracing::debug!(target: "dotfiles::exec", "failed to force-kill child: {err}");
    }
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: nix::sys::signal::Signal) {
    let Ok(pid_raw) = i32::try_from(child.id()) else {
        return;
    };
    let pid = nix::unistd::Pid::from_raw(pid_raw);
    if let Err(err) = nix::sys::signal::killpg(pid, signal) {
        tracing::debug!(target: "dotfiles::exec", "failed to signal process group {pid}: {err}");
    }
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
/// Implementations delegate to real process spawning or provide mock executors
/// for unit tests.
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
    #[allow(
        unused_variables,
        reason = "default implementation intentionally ignores the working directory"
    )]
    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
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
///
/// Uses the default command timeout but has no cancellation token. Production
/// task execution should use [`ManagedExecutor`] so Ctrl-C can stop children.
#[derive(Debug, Clone, Copy, Default)]
#[cfg(any(windows, test, feature = "internal-api"))]
pub struct SystemExecutor;

#[cfg(any(windows, test, feature = "internal-api"))]
impl Executor for SystemExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        execute_checked(cmd, program, &CommandSettings::default_timeout())
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
        execute_checked(
            cmd,
            &format!("{program} in {}", dir.display()),
            &CommandSettings::default_timeout(),
        )
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        let result = execute_unchecked(cmd, program, &CommandSettings::default_timeout())?;
        log_command_output(program, &result);
        Ok(result)
    }

    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args).current_dir(dir);
        let result = execute_unchecked(
            cmd,
            &format!("{program} in {}", dir.display()),
            &CommandSettings::default_timeout(),
        )?;
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

/// Executor used by task execution so spawned commands honour cancellation.
#[derive(Debug, Clone)]
pub struct ManagedExecutor {
    cancellation: CancellationToken,
    timeout: Duration,
}

impl ManagedExecutor {
    /// Create a managed executor with the default per-command timeout.
    #[must_use]
    pub const fn new(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    fn settings(&self) -> CommandSettings {
        CommandSettings::managed(self.cancellation.clone(), self.timeout)
    }
}

impl Executor for ManagedExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        execute_checked(cmd, program, &self.settings())
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
        execute_checked(
            cmd,
            &format!("{program} in {}", dir.display()),
            &self.settings(),
        )
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        let result = execute_unchecked(cmd, program, &self.settings())?;
        log_command_output(program, &result);
        Ok(result)
    }

    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args).current_dir(dir);
        let label = format!("{program} in {}", dir.display());
        let result = execute_unchecked(cmd, &label, &self.settings())?;
        log_command_output(&label, &result);
        Ok(result)
    }

    fn which(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }

    fn which_path(&self, program: &str) -> Result<std::path::PathBuf> {
        which::which(program).with_context(|| format!("{program} not found on PATH"))
    }
}

/// Run a path-addressed command with the smoke-test timeout.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned, times out, or otherwise
/// fails at the process-management layer. Non-zero exit statuses are returned
/// in the [`ExecResult`] for the caller to interpret.
#[cfg(not(windows))]
pub(crate) fn run_path_smoke_test(path: &Path, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = new_command_path(path);
    cmd.args(args);
    let label = path.display().to_string();
    let result = execute_unchecked(cmd, &label, &CommandSettings::timeout(SMOKE_TEST_TIMEOUT))?;
    log_command_output(&label, &result);
    Ok(result)
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

    #[test]
    fn managed_executor_times_out_commands() {
        let token = CancellationToken::new();
        let executor = ManagedExecutor {
            cancellation: token,
            timeout: Duration::from_millis(50),
        };
        #[cfg(windows)]
        let result = executor.run("cmd", &["/C", "ping", "localhost", "-n", "5"]);
        #[cfg(not(windows))]
        let result = executor.run("sh", &["-c", "sleep 5"]);

        let message = result
            .expect_err("long-running command should time out")
            .to_string();
        assert!(
            message.contains("timed out"),
            "timeout error should be explicit: {message}"
        );
    }

    #[test]
    fn managed_executor_cancels_commands() {
        let token = CancellationToken::new();
        token.cancel();
        let executor = ManagedExecutor {
            cancellation: token,
            timeout: Duration::from_secs(5),
        };
        #[cfg(windows)]
        let result = executor.run("cmd", &["/C", "ping", "localhost", "-n", "5"]);
        #[cfg(not(windows))]
        let result = executor.run("sh", &["-c", "sleep 5"]);

        let message = result
            .expect_err("cancelled command should fail")
            .to_string();
        assert!(
            message.contains("cancelled"),
            "cancellation error should be explicit: {message}"
        );
    }
}
