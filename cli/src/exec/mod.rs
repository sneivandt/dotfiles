//! Command execution, output handling, and process-tree management.
use anyhow::{Context, Result, anyhow, bail};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::engine::CancellationToken;

mod output;
mod process;
#[cfg(any(windows, test))]
pub(crate) mod windows;

#[cfg(test)]
use output::stream_summary;
use output::{failure_output, log_command_output};
use process::{terminate_child, wait_after_terminate};

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_mins(30);
#[cfg(not(windows))]
const SMOKE_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

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

#[cfg(windows)]
fn execute_windows_cmd_unchecked(
    command_line: &str,
    settings: &CommandSettings,
) -> Result<ExecResult> {
    use std::os::windows::process::CommandExt as _;

    let mut cmd = new_command("cmd");
    cmd.args(["/D", "/V:OFF", "/S", "/C"]).raw_arg(command_line);
    let result = execute_unchecked(cmd, "cmd", settings)?;
    log_command_output("cmd", &result);
    Ok(result)
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

    /// Execute a pre-quoted `cmd.exe /D /V:OFF /S /C` command line, allowing a
    /// non-zero exit status.
    ///
    /// Real Windows executors append `command_line` with
    /// [`CommandExt::raw_arg`](std::os::windows::process::CommandExt::raw_arg)
    /// so Rust's CRT argument quoting cannot alter `cmd.exe` syntax.
    ///
    /// # Errors
    ///
    /// Returns an error if `cmd.exe` cannot be executed.
    #[cfg(any(windows, test))]
    #[cfg_attr(test, mockall::concretize)]
    fn run_windows_cmd_unchecked(&self, command_line: &str) -> Result<ExecResult> {
        self.run_unchecked("cmd", &["/D", "/V:OFF", "/S", "/C", command_line])
    }

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

    #[cfg(windows)]
    fn run_windows_cmd_unchecked(&self, command_line: &str) -> Result<ExecResult> {
        execute_windows_cmd_unchecked(command_line, &CommandSettings::default_timeout())
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

    #[cfg(windows)]
    fn run_windows_cmd_unchecked(&self, command_line: &str) -> Result<ExecResult> {
        execute_windows_cmd_unchecked(command_line, &self.settings())
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
mod tests;
