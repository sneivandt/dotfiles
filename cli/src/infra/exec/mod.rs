//! Command execution, output handling, and process-tree management.

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// Standard output as UTF-8 string.
    pub stdout: String,
    /// Standard error as UTF-8 string.
    pub stderr: String,
    /// Whether the command exited successfully (status code 0).
    pub success: bool,
    /// Exit code if available, or `None` if terminated by signal.
    pub code: Option<i32>,
}

#[cfg(test)]
impl ExecResult {
    /// Build a successful result with the given standard output.
    #[must_use]
    pub(crate) fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    /// Build an unsuccessful result with the given output and optional exit code.
    #[must_use]
    pub(crate) fn failure(
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        code: Option<i32>,
    ) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            success: false,
            code,
        }
    }
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

/// A typed request describing one child-process invocation.
///
/// Requests are owned so they are straightforward to move into mock
/// expectations and across task-worker threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    command: CommandKind,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
    checked: bool,
    log_arguments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandKind {
    Program(OsString),
    #[cfg(any(windows, test))]
    WindowsCmd(String),
}

impl CommandSpec {
    /// Create a checked command request for `program`.
    #[must_use]
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            command: CommandKind::Program(program.into()),
            args: Vec::new(),
            current_dir: None,
            env: Vec::new(),
            checked: true,
            log_arguments: true,
        }
    }

    /// Create an unchecked, pre-quoted `cmd.exe /D /V:OFF /S /C` request.
    #[cfg(any(windows, test))]
    #[must_use]
    pub(crate) fn windows_cmd(command_line: impl Into<String>) -> Self {
        Self {
            command: CommandKind::WindowsCmd(command_line.into()),
            args: Vec::new(),
            current_dir: None,
            env: Vec::new(),
            checked: false,
            log_arguments: false,
        }
    }

    /// Append one command argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append command arguments.
    #[must_use]
    pub fn args<S: AsRef<OsStr>>(mut self, args: &[S]) -> Self {
        self.args
            .extend(args.iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    /// Set the command's working directory.
    #[must_use]
    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(dir.into());
        self
    }

    /// Add one environment variable override.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Add environment variable overrides.
    #[must_use]
    pub fn envs<K: AsRef<OsStr>, V: AsRef<OsStr>>(mut self, env: &[(K, V)]) -> Self {
        self.env.extend(
            env.iter()
                .map(|(key, value)| (key.as_ref().to_os_string(), value.as_ref().to_os_string())),
        );
        self
    }

    /// Allow a non-zero exit status to be returned as an [`ExecResult`].
    #[must_use]
    pub const fn unchecked(mut self) -> Self {
        self.checked = false;
        self
    }

    /// Suppress arguments in diagnostics for commands whose argv may contain
    /// credentials or other sensitive values.
    #[must_use]
    pub const fn redact_arguments(mut self) -> Self {
        self.log_arguments = false;
        self
    }

    /// Return the executable name.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        match &self.command {
            CommandKind::Program(program) => program,
            #[cfg(any(windows, test))]
            CommandKind::WindowsCmd(_) => OsStr::new("cmd"),
        }
    }

    /// Return the command arguments.
    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.args
    }

    /// Return the configured working directory, if any.
    #[must_use]
    pub fn working_dir(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    /// Return the configured environment overrides.
    #[must_use]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.env
    }

    /// Return whether a non-zero exit is an error.
    #[must_use]
    pub const fn is_checked(&self) -> bool {
        self.checked
    }

    /// Return the pre-quoted Windows command line, when this is a `cmd.exe`
    /// request.
    #[cfg(all(test, windows))]
    #[must_use]
    pub(crate) fn windows_command_line(&self) -> Option<&str> {
        match &self.command {
            CommandKind::WindowsCmd(command_line) => Some(command_line),
            CommandKind::Program(_) => None,
        }
    }

    fn label(&self) -> String {
        let program = render_command_token(self.program());
        let command = if self.args.is_empty() {
            program
        } else if self.log_arguments {
            format!(
                "{program} {}",
                self.args
                    .iter()
                    .map(|arg| render_command_token(arg))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        } else {
            format!("{program} [arguments redacted]")
        };
        self.current_dir.as_ref().map_or_else(
            || command.clone(),
            |dir| format!("{command} (in {})", dir.display()),
        )
    }

    fn into_command(self) -> Command {
        match self.command {
            CommandKind::Program(program) => {
                let mut command = new_command(program);
                command.args(self.args);
                if let Some(dir) = self.current_dir {
                    command.current_dir(dir);
                }
                command.envs(self.env);
                command
            }
            #[cfg(windows)]
            CommandKind::WindowsCmd(command_line) => {
                use std::os::windows::process::CommandExt as _;

                let mut command = new_command("cmd");
                command
                    .args(["/D", "/V:OFF", "/S", "/C"])
                    .raw_arg(command_line);
                command
            }
            #[cfg(all(test, not(windows)))]
            CommandKind::WindowsCmd(command_line) => {
                let mut command = new_command("cmd");
                command.args(["/D", "/V:OFF", "/S", "/C"]).arg(command_line);
                command
            }
        }
    }
}

fn render_command_token(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "/._-:=,@+".contains(character))
    {
        value.into_owned()
    } else {
        format!("{value:?}")
    }
}

/// Typed child-process failures.
#[derive(Debug)]
pub enum ExecError {
    /// The cooperative cancellation token was set.
    Cancelled {
        /// Rendered command label.
        command: String,
        /// Output captured before termination.
        result: ExecResult,
    },
    /// The command exceeded its configured timeout.
    TimedOut {
        /// Rendered command label.
        command: String,
        /// Configured timeout.
        timeout: Duration,
        /// Output captured before termination.
        result: ExecResult,
    },
    /// The operating system could not spawn the command.
    Spawn {
        /// Rendered command label.
        command: String,
        /// Underlying spawn error.
        source: io::Error,
    },
    /// Process management or output capture failed.
    Io {
        /// Rendered command label.
        command: String,
        /// Operation that failed.
        operation: &'static str,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A checked command exited unsuccessfully.
    NonZero {
        /// Rendered command label.
        command: String,
        /// Captured unsuccessful result.
        result: ExecResult,
    },
}

impl fmt::Display for ExecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { command, result } => {
                write!(formatter, "{command} cancelled: {}", failure_output(result))
            }
            Self::TimedOut {
                command,
                timeout,
                result,
            } => write!(
                formatter,
                "{command} timed out after {} seconds: {}",
                timeout.as_secs(),
                failure_output(result)
            ),
            Self::Spawn { command, source } => {
                write!(formatter, "failed to execute {command}: {source}")
            }
            Self::Io {
                command,
                operation,
                source,
            } => write!(formatter, "{operation} for {command}: {source}"),
            Self::NonZero { command, result } => write!(
                formatter,
                "{command} failed (exit {}): {}",
                result.code.unwrap_or(-1),
                failure_output(result)
            ),
        }
    }
}

impl std::error::Error for ExecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Io { source, .. } => Some(source),
            Self::Cancelled { .. } | Self::TimedOut { .. } | Self::NonZero { .. } => None,
        }
    }
}

impl ExecError {
    /// Create an unsuccessful checked-command error, primarily for executor
    /// implementations and mocks.
    #[must_use]
    pub fn non_zero(command: impl Into<String>, result: ExecResult) -> Self {
        Self::NonZero {
            command: command.into(),
            result,
        }
    }

    /// Create a spawn error, primarily for executor implementations and mocks.
    #[must_use]
    pub fn spawn(command: impl Into<String>, source: io::Error) -> Self {
        Self::Spawn {
            command: command.into(),
            source,
        }
    }

    /// Return the rendered command label.
    #[must_use]
    pub fn command(&self) -> &str {
        match self {
            Self::Cancelled { command, .. }
            | Self::TimedOut { command, .. }
            | Self::Spawn { command, .. }
            | Self::Io { command, .. }
            | Self::NonZero { command, .. } => command,
        }
    }

    /// Whether execution stopped because cooperative cancellation was requested.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Return the underlying operating-system error for spawn and process I/O
    /// failures.
    #[must_use]
    pub const fn io_error(&self) -> Option<&io::Error> {
        match self {
            Self::Spawn { source, .. } | Self::Io { source, .. } => Some(source),
            Self::Cancelled { .. } | Self::TimedOut { .. } | Self::NonZero { .. } => None,
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

fn execute_spec(
    spec: CommandSpec,
    settings: &CommandSettings,
) -> std::result::Result<ExecResult, ExecError> {
    let checked = spec.checked;
    let label = spec.label();
    let result = execute_unchecked(spec.into_command(), &label, settings)?;
    log_command_output(&label, &result);
    if checked && !result.success {
        return Err(ExecError::NonZero {
            command: label,
            result,
        });
    }
    Ok(result)
}

fn execute_unchecked(
    mut command: Command,
    label: &str,
    settings: &CommandSettings,
) -> std::result::Result<ExecResult, ExecError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| ExecError::Spawn {
        command: label.to_string(),
        source,
    })?;
    let stdout = child.stdout.take().ok_or_else(|| ExecError::Io {
        command: label.to_string(),
        operation: "capturing stdout",
        source: io::Error::other("child stdout pipe was unavailable"),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| ExecError::Io {
        command: label.to_string(),
        operation: "capturing stderr",
        source: io::Error::other("child stderr pipe was unavailable"),
    })?;
    let (pipe_tx, pipe_rx) = channel::<()>();
    let stdout_reader = spawn_reader(stdout, pipe_tx.clone());
    let stderr_reader = spawn_reader(stderr, pipe_tx);

    let start = Instant::now();
    let mut pipes_closed = false;
    let status = loop {
        if settings.is_cancelled() {
            let result = terminate_and_collect(&mut child, stdout_reader, stderr_reader, label)?;
            return Err(ExecError::Cancelled {
                command: label.to_string(),
                result,
            });
        }
        let remaining = settings.timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            let result = terminate_and_collect(&mut child, stdout_reader, stderr_reader, label)?;
            return Err(ExecError::TimedOut {
                command: label.to_string(),
                timeout: settings.timeout,
                result,
            });
        }

        if let Some(status) = child.try_wait().map_err(|source| ExecError::Io {
            command: label.to_string(),
            operation: "waiting for child",
            source,
        })? {
            break status;
        }

        if pipes_closed {
            std::thread::sleep(POLL_INTERVAL.min(remaining));
        } else if wait_for_pipe_close(&pipe_rx, settings, remaining) {
            pipes_closed = true;
        }
    };

    collect_result(Some(status), stdout_reader, stderr_reader, label)
}

fn terminate_and_collect(
    child: &mut Child,
    stdout_reader: JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: JoinHandle<io::Result<Vec<u8>>>,
    label: &str,
) -> std::result::Result<ExecResult, ExecError> {
    terminate_child(child);
    wait_after_terminate(child);
    let result = collect_result(None, stdout_reader, stderr_reader, label)?;
    log_command_output(label, &result);
    Ok(result)
}

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
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut stream: R,
    done: Sender<()>,
) -> JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let _done = done;
        let mut output = Vec::new();
        stream.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn collect_result(
    status: Option<std::process::ExitStatus>,
    stdout_reader: JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: JoinHandle<io::Result<Vec<u8>>>,
    label: &str,
) -> std::result::Result<ExecResult, ExecError> {
    let stdout = join_reader(stdout_reader, "reading stdout", label)?;
    let stderr = join_reader(stderr_reader, "reading stderr", label)?;
    let success = status.is_some_and(|exit_status| exit_status.success());
    let code = status.and_then(|exit_status| exit_status.code());
    Ok(ExecResult {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        success,
        code,
    })
}

fn join_reader(
    handle: JoinHandle<io::Result<Vec<u8>>>,
    operation: &'static str,
    label: &str,
) -> std::result::Result<Vec<u8>, ExecError> {
    handle.join().map_or_else(
        |_| {
            Err(ExecError::Io {
                command: label.to_string(),
                operation,
                source: io::Error::other("output reader thread panicked"),
            })
        },
        |result| {
            result.map_err(|source| ExecError::Io {
                command: label.to_string(),
                operation,
                source,
            })
        },
    )
}

/// Trait for executing system commands, enabling test injection.
#[cfg_attr(test, mockall::automock)]
pub trait Executor: fmt::Debug + Send + Sync {
    /// Execute one typed command request.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the command is cancelled, times out, cannot
    /// be spawned or managed, or exits non-zero when checked.
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError>;

    /// Check if a program is available on `PATH`.
    #[cfg_attr(not(test), must_use)]
    fn which(&self, program: &str) -> bool;

    /// Resolve the full path of a program on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be found on `PATH`.
    fn which_path(&self, program: &str) -> Result<PathBuf>;
}

/// Executor that spawns real system processes.
#[derive(Debug, Clone)]
pub struct ProcessExecutor {
    settings: CommandSettings,
}

impl ProcessExecutor {
    /// Create an executor that uses the default command timeout and cannot be
    /// cancelled.
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

    /// Managed executor with an explicit timeout.
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
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        execute_spec(spec, &self.settings)
    }

    fn which(&self, program: &str) -> bool {
        resolve_on_path(program).is_some()
    }

    fn which_path(&self, program: &str) -> Result<PathBuf> {
        resolve_on_path(program).ok_or_else(|| anyhow!("{program} not found on PATH"))
    }
}

static PATH_LOOKUPS: LazyLock<Mutex<HashMap<String, Option<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
/// Returns a typed process error. Non-zero exit statuses are returned to the
/// caller for interpretation.
pub(crate) fn run_path_smoke_test(
    path: &Path,
    args: &[&str],
) -> std::result::Result<ExecResult, ExecError> {
    execute_spec(
        CommandSpec::new(path.as_os_str()).args(args).unchecked(),
        &CommandSettings::timeout(SMOKE_TEST_TIMEOUT),
    )
}

/// Run an auxiliary tool with the tool timeout, allowing a non-zero exit.
///
/// # Errors
///
/// Returns a typed process error. Non-zero exit statuses are returned to the
/// caller for interpretation.
pub(crate) fn run_tool_unchecked(
    program: &str,
    args: &[&str],
) -> std::result::Result<ExecResult, ExecError> {
    execute_spec(
        CommandSpec::new(program).args(args).unchecked(),
        &CommandSettings::timeout(TOOL_TIMEOUT),
    )
}

#[cfg(test)]
mod tests;
