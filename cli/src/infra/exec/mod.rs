//! Command execution, output handling, and process-tree management.
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::infra::cancellation::CancellationToken;

mod output;
mod process;
#[cfg(any(windows, test))]
pub(crate) mod windows;

#[cfg(test)]
use output::stream_summary;
use output::{failure_output, log_command_output};
use process::{terminate_child, wait_after_terminate};

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_mins(30);
const TOOL_TIMEOUT: Duration = Duration::from_mins(2);
const SMOKE_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Create a new [`Command`] with platform-appropriate defaults.
///
/// Accepts anything that names a program — a bare `&str` looked up on `PATH`
/// or a fully qualified `&Path` — so there is a single place where the
/// platform defaults below are applied.
///
/// On Unix the child is placed in its own process group so that a
/// `SIGINT` from Ctrl-C reaches only the Rust process (via the
/// cooperative cancellation token).  The executor can then terminate the
/// whole child process group on cancellation or timeout.
fn new_command<S: AsRef<OsStr>>(program: S) -> Command {
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
    const fn default_timeout() -> Self {
        Self {
            timeout: DEFAULT_COMMAND_TIMEOUT,
            cancellation: None,
        }
    }

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
    let (pipe_tx, pipe_rx) = channel::<()>();
    let stdout_reader = spawn_reader(stdout, "stdout", label, pipe_tx.clone());
    let stderr_reader = spawn_reader(stderr, "stderr", label, pipe_tx);

    let start = Instant::now();
    let mut pipes_closed = false;
    let status = loop {
        if settings.is_cancelled() {
            let result = terminate_and_collect(&mut child, stdout_reader, stderr_reader, label)?;
            bail!("{label} cancelled: {}", failure_output(&result));
        }
        let remaining = settings.timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            let result = terminate_and_collect(&mut child, stdout_reader, stderr_reader, label)?;
            bail!(
                "{label} timed out after {} seconds: {}",
                settings.timeout.as_secs(),
                failure_output(&result)
            );
        }

        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("waiting for: {label}"))?
        {
            break status;
        }

        if pipes_closed {
            // Rare: the child closed both pipes but has not exited yet (for
            // example it forked a grandchild that inherited them). There is no
            // remaining event to block on, so fall back to polling.
            std::thread::sleep(POLL_INTERVAL.min(remaining));
        } else if wait_for_pipe_close(&pipe_rx, settings, remaining) {
            pipes_closed = true;
        }
    };

    collect_result(Some(status), stdout_reader, stderr_reader)
}

fn terminate_and_collect(
    child: &mut Child,
    stdout_reader: JoinHandle<Result<Vec<u8>>>,
    stderr_reader: JoinHandle<Result<Vec<u8>>>,
    label: &str,
) -> Result<ExecResult> {
    #[cfg(unix)]
    terminate_child(child);
    #[cfg(windows)]
    terminate_child(child);
    wait_after_terminate(child);
    let result = collect_result(None, stdout_reader, stderr_reader)?;
    log_command_output(label, &result);
    Ok(result)
}

/// Block until the child closes both output pipes, or until it is time to
/// re-check cancellation and the timeout.
///
/// Returns `true` once every reader has finished, which happens as the child
/// exits and is therefore the signal that [`Child::try_wait`] is about to
/// succeed. Waiting on that event instead of sleeping a fixed interval removes
/// the wake-up latency that a poll loop adds to every single command.
///
/// A run with a cancellation token still needs periodic wake-ups because
/// [`CancellationToken`] is a polled flag with nothing to block on; without one
/// the whole remaining timeout can be spent in a single blocking wait.
fn wait_for_pipe_close(
    pipe_rx: &Receiver<()>,
    settings: &CommandSettings,
    remaining: Duration,
) -> bool {
    let slice = if settings.cancellation.is_some() {
        POLL_INTERVAL.min(remaining)
    } else {
        remaining
    };
    match pipe_rx.recv_timeout(slice) {
        Err(RecvTimeoutError::Timeout) => false,
        // Nothing is ever sent on this channel: every reader holds a sender and
        // drops it on completion, so disconnection *is* the completion signal.
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
    }
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

/// Drain a child output stream on its own thread.
///
/// `done` is moved into the thread and dropped when reading finishes, so the
/// receiver observes a disconnect once both readers are complete. That is the
/// event [`execute_unchecked`] blocks on instead of polling the child.
fn spawn_reader<R: Read + Send + 'static>(
    mut stream: R,
    name: &'static str,
    label: &str,
    done: Sender<()>,
) -> JoinHandle<Result<Vec<u8>>> {
    let label = label.to_string();
    std::thread::spawn(move || {
        let _done = done;
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
    /// # Errors
    ///
    /// Returns an error if the command fails to execute or cannot be found,
    /// but does NOT fail on non-zero exit codes (which are captured in the result).
    #[cfg_attr(test, mockall::concretize)]
    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult>;

    /// Check if a program is available on PATH.
    #[cfg_attr(not(test), must_use)]
    fn which(&self, program: &str) -> bool;

    /// Resolve the full path of a program on PATH.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be found on PATH.
    fn which_path(&self, program: &str) -> Result<PathBuf>;
}

/// Executor that spawns real system processes.
///
/// The only thing that varies between uses is the [`CommandSettings`] applied
/// to each spawn, so both flavours are the same type: [`system`](Self::system)
/// spawns with the default timeout and no cancellation, while
/// [`managed`](Self::managed) additionally honours a cancellation token so
/// Ctrl-C can stop children.
#[derive(Debug, Clone)]
pub struct ProcessExecutor {
    settings: CommandSettings,
}

impl ProcessExecutor {
    /// Create an executor that uses the default command timeout and cannot be
    /// cancelled.
    ///
    /// Production task execution should prefer [`managed`](Self::managed) so
    /// Ctrl-C can stop spawned children; this flavour exists for the
    /// bootstrap paths that run before a cancellation token exists.
    #[must_use]
    pub const fn system() -> Self {
        Self {
            settings: CommandSettings::default_timeout(),
        }
    }

    /// Create an executor whose spawned commands honour `cancellation`.
    #[must_use]
    pub const fn managed(cancellation: CancellationToken) -> Self {
        Self {
            settings: CommandSettings::managed(cancellation, DEFAULT_COMMAND_TIMEOUT),
        }
    }

    /// Managed executor with an explicit timeout, for tests that need to
    /// observe timeout and cancellation behaviour without waiting minutes.
    #[cfg(test)]
    const fn managed_with_timeout(cancellation: CancellationToken, timeout: Duration) -> Self {
        Self {
            settings: CommandSettings::managed(cancellation, timeout),
        }
    }
}

impl Default for ProcessExecutor {
    fn default() -> Self {
        Self::system()
    }
}

impl Executor for ProcessExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        execute_checked(cmd, program, &self.settings)
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
            &self.settings,
        )
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args);
        let result = execute_unchecked(cmd, program, &self.settings)?;
        log_command_output(program, &result);
        Ok(result)
    }

    #[cfg(windows)]
    fn run_windows_cmd_unchecked(&self, command_line: &str) -> Result<ExecResult> {
        execute_windows_cmd_unchecked(command_line, &self.settings)
    }

    fn run_unchecked_in(&self, dir: &Path, program: &str, args: &[&str]) -> Result<ExecResult> {
        let mut cmd = new_command(program);
        cmd.args(args).current_dir(dir);
        let label = format!("{program} in {}", dir.display());
        let result = execute_unchecked(cmd, &label, &self.settings)?;
        log_command_output(&label, &result);
        Ok(result)
    }

    fn which(&self, program: &str) -> bool {
        resolve_on_path(program).is_some()
    }

    fn which_path(&self, program: &str) -> Result<PathBuf> {
        resolve_on_path(program).ok_or_else(|| anyhow!("{program} not found on PATH"))
    }
}

/// Memoized `PATH` lookups, keyed by program name.
///
/// `which::which` walks every `PATH` entry and stats candidate files. Task
/// applicability gates call it repeatedly for the same handful of programs
/// within a single run, and `PATH` does not change while the process is alive,
/// so the answer is cached process-wide.
static PATH_LOOKUPS: LazyLock<Mutex<HashMap<String, Option<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Resolve `program` on `PATH`, reusing a previously cached answer.
///
/// A poisoned lock is recovered rather than propagated: the cache holds only
/// derived data, so a panic in another thread cannot leave it inconsistent.
fn resolve_on_path(program: &str) -> Option<PathBuf> {
    let mut cache = PATH_LOOKUPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(cached) = cache.get(program) {
        return cached.clone();
    }
    let resolved = which::which(program).ok();
    cache.insert(program.to_string(), resolved.clone());
    resolved
}

/// Run a path-addressed command with the smoke-test timeout.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned, times out, or otherwise
/// fails at the process-management layer. Non-zero exit statuses are returned
/// in the [`ExecResult`] for the caller to interpret.
pub(crate) fn run_path_smoke_test(path: &Path, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = new_command(path);
    cmd.args(args);
    let label = path.display().to_string();
    let result = execute_unchecked(cmd, &label, &CommandSettings::timeout(SMOKE_TEST_TIMEOUT))?;
    log_command_output(&label, &result);
    Ok(result)
}

/// Run an auxiliary tool with the tool timeout, allowing a non-zero exit.
///
/// Used by subsystems that run outside task execution (for example
/// self-update provenance verification) and therefore have no
/// [`Executor`] available on every platform.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or times out. Non-zero
/// exit statuses are returned in the [`ExecResult`] for the caller to
/// interpret.
pub(crate) fn run_tool_unchecked(program: &str, args: &[&str]) -> Result<ExecResult> {
    let mut cmd = new_command(program);
    cmd.args(args);
    let settings = CommandSettings {
        timeout: TOOL_TIMEOUT,
        cancellation: None,
    };
    let result = execute_unchecked(cmd, program, &settings)?;
    log_command_output(program, &result);
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
