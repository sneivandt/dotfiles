//! The run log: a single append-only record of everything that happened.
//!
//! Every message is written here immediately, in true chronological order,
//! regardless of whether it is also shown on the console.  Console output is a
//! separate concern handled by the tracing console layer, so this module never
//! needs to know about verbosity, styling, or terminal state.
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::utils::{format_utc_datetime_us, strip_ansi};

thread_local! {
    /// Task name for the current thread, set by the parallel scheduler.
    ///
    /// `std::thread::scope` spawns unnamed threads, so
    /// `std::thread::current().name()` returns `None`.  This thread-local
    /// stores the task name so that [`RunLog::emit`] can identify which task
    /// produced each event without needing tracing spans.
    static LOG_TASK_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the run-log task name for the current thread.
///
/// Called by the parallel scheduler when a thread starts working on a task.
/// The name is used by [`RunLog::emit`] as a fallback when the OS thread has
/// no name.
pub fn set_log_thread_name(name: &str) {
    LOG_TASK_NAME.with(|cell| {
        *cell.borrow_mut() = Some(name.to_string());
    });
}

/// Guard that restores the previous run-log task context when dropped.
#[derive(Debug)]
pub struct LogTaskContextGuard {
    previous: Option<String>,
}

impl Drop for LogTaskContextGuard {
    fn drop(&mut self) {
        LOG_TASK_NAME.with(|cell| {
            *cell.borrow_mut() = self.previous.take();
        });
    }
}

/// Set the run-log task context for the current scope.
#[must_use]
pub fn log_task_context(name: &str) -> LogTaskContextGuard {
    let previous = LOG_TASK_NAME.with(|cell| cell.borrow_mut().replace(name.to_string()));
    LogTaskContextGuard { previous }
}

/// Read the run-log task name for the current thread.
#[must_use]
pub fn log_thread_name() -> String {
    if let Some(name) = LOG_TASK_NAME.with(|cell| cell.borrow().clone()) {
        return name;
    }
    let thread = std::thread::current();
    if let Some(name) = thread.name() {
        return name.to_string();
    }
    "?".to_string()
}

/// Event kinds for the run log.
///
/// Each variant maps to a stable `snake_case` event name in the log output.
#[derive(Debug, Clone, Copy)]
pub enum LogEvent {
    /// Informational message from a task.
    Info,
    /// Debug-level message.
    Debug,
    /// Warning message.
    Warn,
    /// Error message.
    Error,
    /// Stage header (major section).
    Stage,
    /// Dry-run preview.
    DryRun,
    /// A task thread has been spawned and is waiting for dependencies.
    TaskWait,
    /// A task's dependencies are satisfied; execution begins.
    TaskStart,
    /// A task finished executing.
    TaskDone,
    /// A task was skipped (not applicable).
    TaskSkip,
    /// A task failed (e.g. returned an error or panicked).
    TaskFail,
    /// How long a task spent executing.
    TaskTiming,
    /// Resource state check.
    ResourceCheck,
    /// Resource apply (mutation).
    ResourceApply,
    /// Resource apply result.
    ResourceResult,
    /// Resource removal.
    ResourceRemove,
}

impl LogEvent {
    /// Stable event name for the log line.
    const fn name(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Stage => "stage",
            Self::DryRun => "dry_run",
            Self::TaskWait => "task_wait",
            Self::TaskStart => "task_start",
            Self::TaskDone => "task_done",
            Self::TaskSkip => "task_skip",
            Self::TaskFail => "task_fail",
            Self::TaskTiming => "task_timing",
            Self::ResourceCheck => "resource_check",
            Self::ResourceApply => "resource_apply",
            Self::ResourceResult => "resource_result",
            Self::ResourceRemove => "resource_remove",
        }
    }

    /// Map a raw [`tracing`] level onto a run-log event kind.
    ///
    /// Used by the tracing bridge so that `tracing::debug!` / `warn!` calls
    /// made directly by `infra` and `domains` code land in the run log with a
    /// sensible event name.
    pub(super) const fn from_level(level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::INFO => Self::Info,
            tracing::Level::DEBUG | tracing::Level::TRACE => Self::Debug,
        }
    }
}

/// The run log: every event, in the order it actually happened.
///
/// Written to `$XDG_CACHE_HOME/dotfiles/<command>.log`.  Each line records a
/// sequence number, microsecond-precision elapsed time from program start, a
/// wall-clock timestamp, the originating task/thread context, and the event
/// kind, which together make it possible to reconstruct the true interleaved
/// timeline of parallel execution.
///
/// Entries are written immediately as they are produced.  This is deliberately
/// decoupled from console rendering: the console buffers per-task output to
/// keep parallel runs readable, whereas the run log always reflects real
/// chronological order.
#[derive(Debug)]
pub struct RunLog {
    file: Mutex<fs::File>,
    #[cfg(test)]
    path: PathBuf,
    start: Instant,
    sequence: AtomicU64,
}

impl RunLog {
    /// Create the run log for `command` in the resolved dotfiles cache
    /// directory.
    ///
    /// Returns `None` if the cache directory or file cannot be created, in
    /// which case the run simply proceeds without a file log.
    pub(super) fn create(command: &str, start: Instant) -> Option<Self> {
        Self::new(command, &super::utils::dotfiles_cache_dir()?, start)
    }

    /// Create a new run log file for the given command.
    ///
    /// `cache_dir` is the resolved `dotfiles` cache directory (e.g.
    /// `$XDG_CACHE_HOME/dotfiles/`).  The caller is responsible for
    /// resolving the directory; this constructor never reads environment
    /// variables.
    ///
    /// Returns `None` if the file cannot be created.
    pub(super) fn new(command: &str, cache_dir: &Path, start: Instant) -> Option<Self> {
        let path = cache_dir.join(format!("{command}.log"));
        let version =
            option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION")));
        let header = format!(
            "# Dotfiles {version} {}\n\
             # Columns: seq | elapsed_us | wall_utc | context | event | message\n",
            format_utc_datetime_us(),
        );
        fs::write(&path, header).ok()?;
        let file = fs::OpenOptions::new().append(true).open(&path).ok()?;
        Some(Self {
            file: Mutex::new(file),
            #[cfg(test)]
            path,
            start,
            sequence: AtomicU64::new(0),
        })
    }

    /// Emit an event, attributing it to the current thread's task context.
    ///
    /// Each line is:
    /// `<seq> +<elapsed_us> <wall_utc_us> [<context>] [<event>] <message>`
    ///
    /// ANSI escape sequences are stripped from the message. The context comes
    /// from the current task context when one is set, otherwise from the OS
    /// thread name when available (e.g. `"main"`). Blank messages are omitted.
    pub fn emit(&self, event: LogEvent, message: &str) {
        if let Some(formatted_message) = format_log_message(message) {
            self.write_event(event, &log_thread_name(), &formatted_message);
        }
    }

    /// Emit an event with an explicit context name.
    fn emit_with_context(&self, event: LogEvent, context: &str, message: &str) {
        let Some(formatted_message) = format_log_message(message) else {
            return;
        };
        self.write_event(event, context, &formatted_message);
    }

    fn write_event(&self, event: LogEvent, context: &str, formatted_message: &str) {
        let Ok(mut f) = self.file.lock() else {
            return;
        };
        let seq = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let elapsed = self.start.elapsed();
        let elapsed_us = elapsed.as_micros();
        let wall = format_utc_datetime_us();
        let event_name = event.name();
        let line = format!(
            "{seq:06} +{elapsed_us:>12} {wall} [{context}] [{event_name}] {formatted_message}\n"
        );
        drop(f.write_all(line.as_bytes()));
    }

    /// Emit an event with an explicit task name context.
    pub fn emit_task(&self, event: LogEvent, task: &str, message: &str) {
        self.emit_with_context(event, task, message);
    }

    /// Return the path of the run log file.
    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn format_log_message(message: &str) -> Option<String> {
    let clean = strip_ansi(message);
    let mut formatted = String::new();
    for line in clean.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !formatted.is_empty() {
            formatted.push_str(" | ");
        }
        formatted.push_str(line);
    }
    if formatted.is_empty() {
        None
    } else {
        Some(formatted)
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
    use std::fs;

    fn isolated_run_log() -> (RunLog, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run_log = RunLog::new("test", tmp.path(), Instant::now()).expect("run log");
        (run_log, tmp)
    }

    #[test]
    fn run_log_is_created() {
        let (run_log, _tmp) = isolated_run_log();
        assert!(run_log.path().exists(), "run log file should be created");
    }

    #[test]
    fn run_log_has_header() {
        let (run_log, _tmp) = isolated_run_log();
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.starts_with("# Dotfiles "),
            "run log should start with header"
        );
        assert!(
            contents.contains("seq | elapsed_us | wall_utc | context | event | message"),
            "header should describe columns"
        );
    }

    #[test]
    fn run_log_emit_writes_event() {
        let (run_log, _tmp) = isolated_run_log();
        let marker = format!("run-log-marker-{}", std::process::id());
        run_log.emit(LogEvent::Info, &marker);
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(contents.contains(&marker), "event should appear in run log");
        assert!(
            contents.contains("[info]"),
            "run-log event should include info event name"
        );
    }

    #[test]
    fn run_log_event_appears_after_context_without_padding() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Warn, "event-order");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let line = contents
            .lines()
            .find(|l| l.contains("event-order"))
            .unwrap();
        assert!(
            line.contains("] [warn] event-order"),
            "event should be bracketed immediately before the message: {line}"
        );
        assert!(
            !line.contains("[warn]  event-order"),
            "event column should not add padding after the closing bracket: {line}"
        );
    }

    #[test]
    fn run_log_emit_has_microsecond_precision() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Stage, "precision-test");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let has_us = contents
            .lines()
            .any(|l| l.contains("precision-test") && l.contains('T') && l.contains('Z'));
        assert!(
            has_us,
            "run-log should contain microsecond wall-clock timestamp"
        );
    }

    #[test]
    fn run_log_emit_task_includes_task_name() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_task(LogEvent::TaskStart, "Install symlinks", "deps satisfied");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.contains("[Install symlinks]"),
            "run-log task event should include task name in brackets"
        );
        assert!(
            contents.contains("deps satisfied"),
            "run-log task event should include the message"
        );
    }

    #[test]
    fn run_log_resource_events_include_messages() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::ResourceCheck, "~/.bashrc state=Missing");
        run_log.emit(LogEvent::ResourceApply, "link ~/.bashrc");
        run_log.emit(LogEvent::ResourceResult, "~/.bashrc applied");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(contents.contains("~/.bashrc state=Missing"));
        assert!(contents.contains("link ~/.bashrc"));
        assert!(contents.contains("~/.bashrc applied"));
    }

    #[test]
    fn run_log_events_are_chronologically_ordered() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Stage, "first");
        std::thread::sleep(std::time::Duration::from_millis(1));
        run_log.emit(LogEvent::Info, "second");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let first_pos = contents.find("first").expect("first in log");
        let second_pos = contents.find("second").expect("second in log");
        assert!(
            first_pos < second_pos,
            "events should appear in chronological order"
        );
    }

    #[test]
    fn log_event_names_are_stable_snake_case() {
        assert_eq!(LogEvent::Debug.name(), "debug");
        assert_eq!(LogEvent::Info.name(), "info");
        assert_eq!(LogEvent::Stage.name(), "stage");
        assert_eq!(LogEvent::TaskStart.name(), "task_start");
        assert_eq!(LogEvent::TaskDone.name(), "task_done");
        assert_eq!(LogEvent::ResourceApply.name(), "resource_apply");
        assert_eq!(LogEvent::Warn.name(), "warn");
        assert_eq!(LogEvent::Error.name(), "error");
        assert_eq!(LogEvent::TaskFail.name(), "task_fail");
    }

    #[test]
    fn log_thread_name_returns_nonempty() {
        let name = log_thread_name();
        assert!(
            !name.is_empty(),
            "log_thread_name should never return empty string"
        );
    }

    #[test]
    fn set_log_thread_name_is_retrieved_on_unnamed_thread() {
        let result = std::thread::spawn(|| {
            set_log_thread_name("my-task");
            log_thread_name()
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, "my-task");
    }

    #[test]
    fn log_task_context_restores_previous_name() {
        let result = std::thread::spawn(|| {
            set_log_thread_name("outer-task");
            let inner = {
                let _guard = log_task_context("inner-task");
                log_thread_name()
            };
            (inner, log_thread_name())
        })
        .join()
        .expect("thread should not panic");
        assert_eq!(result, ("inner-task".to_string(), "outer-task".to_string()));
    }

    #[test]
    fn run_log_omits_blank_messages() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit_task(LogEvent::TaskDone, "task-name", "");
        run_log.emit(LogEvent::Debug, "   \t");
        run_log.emit(LogEvent::Info, "after blanks");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            !contents.contains("[task-name]"),
            "empty messages should be omitted"
        );
        assert!(
            contents
                .lines()
                .any(|line| line.starts_with("000001 ") && line.contains("after blanks")),
            "blank messages should not consume sequence numbers"
        );
    }

    #[test]
    fn run_log_collapses_multiline_message_without_blank_lines() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "first\n\n  second  ");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        let line = contents
            .lines()
            .find(|line| line.contains("first"))
            .unwrap();
        assert!(
            line.ends_with("first | second"),
            "multiline messages should be collapsed: {line}"
        );
        assert!(
            !contents.lines().any(str::is_empty),
            "run-log log should not contain blank lines"
        );
    }

    #[test]
    fn run_log_events_have_sequence_numbers() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "first");
        run_log.emit(LogEvent::Info, "second");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.lines().any(|line| line.starts_with("000001 ")),
            "first event should have sequence 1"
        );
        assert!(
            contents.lines().any(|line| line.starts_with("000002 ")),
            "second event should have sequence 2"
        );
    }

    #[test]
    fn run_log_strips_ansi_from_message() {
        let (run_log, _tmp) = isolated_run_log();
        run_log.emit(LogEvent::Info, "\x1b[31mred-message\x1b[0m");
        let contents = fs::read_to_string(run_log.path()).unwrap();
        assert!(
            contents.contains("red-message"),
            "stripped text should appear"
        );
        assert!(
            !contents.contains("\x1b[31m"),
            "ANSI codes should be stripped"
        );
    }
}
