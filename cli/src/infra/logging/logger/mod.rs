//! Structured logger with dry-run awareness and summary collection.
//!
//! The implementation is split across submodules by responsibility:
//! - This file: [`Logger`] struct, constructors, accessors, message-emitting
//!   methods (info/debug/warn/error/stage/etc.), task recording, and the
//!   [`Output`] / [`TaskRecorder`] trait impls.
//! - [`summary`]: end-of-run [`print_summary`](Logger::print_summary).
//! - [`progress`]: transient live status rendering.
//! - [`notifications`]: parallel-task lifecycle hooks and live status redraws.

mod notifications;
mod progress;
mod summary;

pub(in crate::infra::logging) use progress::stdout_supports_progress;

#[cfg(test)]
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU16;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::diagnostic::{DiagEvent, DiagnosticLog};
use super::types::{Output, TaskEntry, TaskRecorder, TaskStatus};
use super::utils::dotfiles_cache_dir;
#[cfg(test)]
use super::utils::{dotfiles_cache_subdir, log_file_path_in};

/// Generate an inherent `pub fn $name(&self, msg: &str)` method on `Logger`
/// that optionally emits to the diagnostic log and then forwards to the given
/// `tracing` macro.
///
/// Two forms are supported:
/// - Without a target: `log_method!(#[doc...] name, Event, tracing::mac)`
/// - With a target:    `log_method!(#[doc...] name, Event, tracing::mac, target: "…")`
macro_rules! log_method {
    ($(#[$doc:meta])* $name:ident, $event:ident, $mac:path) => {
        $(#[$doc])*
        pub fn $name(&self, msg: &str) {
            if let Some(d) = &self.diagnostic {
                d.emit(DiagEvent::$event, msg);
            }
            $mac!("{msg}");
        }
    };
    ($(#[$doc:meta])* $name:ident, $event:ident, $mac:path, target: $target:literal) => {
        $(#[$doc])*
        pub fn $name(&self, msg: &str) {
            if let Some(d) = &self.diagnostic {
                d.emit(DiagEvent::$event, msg);
            }
            $mac!(target: $target, "{msg}");
        }
    };
}

/// Implement the display methods of [`Output`] by delegating to inherent methods
/// of the same name on the implementing type.
///
/// The `record_task` method is **not** included because its signature differs
/// from the `fn(&self, &str)` pattern shared by the display methods.
macro_rules! forward_log_methods {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method(&self, msg: &str) {
                self.$method(msg);
            }
        )+
    };
}

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent log file at
/// `$XDG_CACHE_HOME/dotfiles/<command>.log` (default `~/.cache/dotfiles/<command>.log`)
/// with timestamps and ANSI codes stripped, regardless of the verbose flag.
#[derive(Debug)]
pub struct Logger {
    /// Command currently being executed (`install`, `update`, etc.).
    pub(super) command: String,
    pub(super) tasks: Mutex<Vec<TaskEntry>>,
    pub(super) task_details: Mutex<Vec<TaskDetailEntry>>,
    #[cfg(test)]
    pub(super) log_file: Option<PathBuf>,
    /// Serializes console output from parallel task flushes.
    pub(super) flush_lock: Mutex<()>,
    /// Names of tasks currently executing in parallel.
    pub(super) active_tasks: Mutex<Vec<String>>,
    /// Number of transient rows currently displayed.
    ///
    /// The transient status area is redrawn from whole rows. Each row is
    /// truncated to fit the terminal, avoiding wrapped-row cursor arithmetic.
    pub(super) progress_rows: AtomicU16,
    /// Whether the bottom row in the transient status area is the active-task row.
    pub(super) status_row_visible: AtomicBool,
    /// Whether any completed task has emitted durable console output.
    pub(super) task_console_output_emitted: AtomicBool,
    /// High-precision diagnostic log; `None` when the cache dir is unavailable.
    pub(super) diagnostic: Option<DiagnosticLog>,
    /// Instant when the logger was created, used for elapsed time in summary.
    pub(super) start: Instant,
    /// Whether verbose output is enabled (show all stage headers and info).
    pub(super) verbose: bool,
    /// Whether the current command is previewing changes without applying them.
    pub(super) dry_run: bool,
    /// Whether the separator after startup metadata has been emitted.
    startup_separator_emitted: AtomicBool,
}

/// Buffered user-facing detail lines emitted by a completed task.
#[derive(Debug, Clone)]
pub(in crate::infra::logging) struct TaskDetailEntry {
    /// Human-readable task name.
    pub(super) name: String,
    /// Detail lines emitted by the task while it ran.
    pub(super) lines: Vec<String>,
}

impl Logger {
    /// Create a new logger.
    ///
    /// Stores the log file path for display in the run summary.  The log file
    /// itself is created and initialised by [`init_subscriber`](super::subscriber::init_subscriber) via
    /// [`FileLayer`](super::subscriber::FileLayer); this constructor does not write to the file.
    #[must_use]
    pub fn new(command: &str) -> Self {
        let start = Instant::now();
        Self {
            command: command.to_string(),
            tasks: Mutex::new(Vec::new()),
            task_details: Mutex::new(Vec::new()),
            #[cfg(test)]
            log_file: super::utils::log_file_path(command),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: AtomicU16::new(0),
            status_row_visible: AtomicBool::new(false),
            task_console_output_emitted: AtomicBool::new(false),
            diagnostic: dotfiles_cache_dir()
                .and_then(|dir| DiagnosticLog::new(command, &dir, start)),
            start,
            verbose: true,
            dry_run: false,
            startup_separator_emitted: AtomicBool::new(false),
        }
    }

    /// Set the verbose mode on this logger.
    ///
    /// Also updates the global [`subscriber`](super::subscriber) flag so the
    /// console formatter and file layer stay in sync.
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
        super::subscriber::set_verbose(verbose);
    }

    /// Set dry-run mode on this logger for summary rendering.
    pub const fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
    }

    /// Create a new logger using an explicit cache base directory.
    ///
    /// Like [`new`](Self::new) but resolves the log file path and diagnostic
    /// log under `cache_dir` instead of reading `XDG_CACHE_HOME` from the
    /// environment.  Intended for tests that need an isolated logger without
    /// mutating process-global state.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_in(command: &str, cache_dir: &std::path::Path) -> Self {
        let start = Instant::now();
        Self {
            command: command.to_string(),
            tasks: Mutex::new(Vec::new()),
            task_details: Mutex::new(Vec::new()),
            log_file: log_file_path_in(command, cache_dir),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: AtomicU16::new(0),
            status_row_visible: AtomicBool::new(false),
            task_console_output_emitted: AtomicBool::new(false),
            diagnostic: dotfiles_cache_subdir(cache_dir)
                .and_then(|dir| DiagnosticLog::new(command, &dir, start)),
            start,
            verbose: true,
            dry_run: false,
            startup_separator_emitted: AtomicBool::new(false),
        }
    }

    /// Return the diagnostic log, if available.
    pub const fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }

    /// Return whether verbose output mode is enabled.
    pub const fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Return the log file path, if available.
    #[cfg(test)]
    #[must_use]
    pub const fn log_path(&self) -> Option<&PathBuf> {
        self.log_file.as_ref()
    }

    /// Return a clone of all recorded task entries (test-only).
    #[cfg(test)]
    pub(crate) fn task_entries(&self) -> Vec<TaskEntry> {
        self.tasks.lock().map_or_else(|_| vec![], |g| g.clone())
    }

    /// Return the current value of `progress_rows` (test-only).
    #[cfg(test)]
    pub(crate) fn progress_rows_count(&self) -> u16 {
        self.progress_rows.load(Ordering::Relaxed)
    }

    /// Return whether the active-task status row is currently displayed (test-only).
    #[cfg(test)]
    pub(crate) fn status_row_visible(&self) -> bool {
        self.status_row_visible.load(Ordering::Relaxed)
    }

    /// Return whether task console output has been emitted (test-only).
    #[cfg(test)]
    pub(crate) fn task_console_output_emitted(&self) -> bool {
        self.task_console_output_emitted.load(Ordering::Relaxed)
    }

    log_method!(
        /// Log an error message.
        error, Error, tracing::error
    );

    log_method!(
        /// Log a warning message.
        warn, Warn, tracing::warn
    );

    log_method!(
        /// Log a stage header (major section).
        stage, Stage, tracing::info, target: "dotfiles::stage"
    );

    log_method!(
        /// Log an informational message.
        info, Info, tracing::info
    );

    log_method!(
        /// Log a debug message (suppressed on console unless verbose; always
        /// written to the log file via the [`FileLayer`](super::subscriber::FileLayer)).
        debug, Debug, tracing::debug
    );

    log_method!(
        /// Log a dry-run action message.
        dry_run, DryRun, tracing::info, target: "dotfiles::dry_run"
    );

    log_method!(
        /// Log a message that always appears on the console regardless of verbose setting.
        always, Info, tracing::info, target: "dotfiles::always"
    );

    log_method!(
        /// Log a compact task-result line (console-only, omitted from the log file).
        task_result, Info, tracing::info, target: "dotfiles::task_result"
    );

    /// Record a task result for the summary.
    pub fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        if let Ok(mut guard) = self.tasks.lock() {
            guard.push(TaskEntry {
                name: name.to_string(),
                status,
                message: message.map(String::from),
            });
        }
    }

    /// Record buffered user-facing detail lines for a completed task.
    pub(in crate::infra::logging) fn record_task_details(&self, name: &str, lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.task_details.lock() {
            guard.push(TaskDetailEntry {
                name: name.to_string(),
                lines,
            });
        }
    }

    /// Count the number of failed tasks.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.tasks.lock().map_or(0, |guard| {
            guard
                .iter()
                .filter(|t| t.status == TaskStatus::Failed)
                .count()
        })
    }

    /// Emit the single blank line that separates startup metadata from details.
    pub fn separate_from_startup(&self) {
        if !self.startup_separator_emitted.swap(true, Ordering::Relaxed) {
            self.always("");
        }
    }
}

impl Output for Logger {
    forward_log_methods!(stage, info, debug, warn, error, dry_run, always);

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }
}

impl TaskRecorder for Logger {
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, status, message);
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
    use crate::infra::logging::isolated_logger;
    use crate::infra::logging::types::Log;
    use std::fs;

    #[test]
    fn logger_new() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(log.task_entries().is_empty(), "expected empty task list");
    }

    #[test]
    fn record_task_ok() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("symlinks", TaskStatus::Ok, None);
        let tasks = log.task_entries();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
    }

    #[test]
    fn record_task_with_message() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("packages", TaskStatus::Skipped, Some("not on arch"));
        assert_eq!(
            log.task_entries()[0].message,
            Some("not on arch".to_string())
        );
    }

    #[test]
    fn record_multiple_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskStatus::DryRun, None);
        assert_eq!(log.task_entries().len(), 3);
    }

    #[test]
    fn log_file_is_created() {
        let (log, _tmp, _guard) = isolated_logger();
        let path = log.log_path().expect("log path should exist");
        assert!(path.exists(), "log file should be created on Logger::new");
    }

    #[test]
    fn debug_always_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("debug-marker-{}", std::process::id());
        log.debug(&marker);
        let path = log.log_path().expect("log path should exist");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&marker),
            "debug messages should always appear in the log file"
        );
    }

    #[test]
    fn failure_count_returns_correct_count() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.failure_count(), 0);
        log.record_task("a", TaskStatus::Ok, None);
        log.record_task("b", TaskStatus::Failed, Some("error 1"));
        log.record_task("c", TaskStatus::Failed, Some("error 2"));
        log.record_task("d", TaskStatus::Skipped, None);
        assert_eq!(log.failure_count(), 2);
    }

    #[test]
    fn log_trait_delegates_to_logger() {
        let (log, _tmp, _guard) = isolated_logger();
        let log_ref: &dyn Log = &log;
        log_ref.record_task("via-trait", TaskStatus::Ok, None);
        assert_eq!(log.task_entries().len(), 1);
    }

    #[test]
    fn diagnostic_log_accessible_via_trait() {
        let (log, _tmp, _guard) = isolated_logger();
        let log_ref: &dyn Log = &log;
        assert!(
            log_ref.diagnostic().is_some(),
            "diagnostic() should be accessible via Log trait"
        );
    }

    #[test]
    fn info_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("info-marker-{}", std::process::id());
        log.info(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&marker),
            "info message should appear in log file"
        );
    }

    #[test]
    fn warn_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("warn-marker-{}", std::process::id());
        log.warn(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains("[warn]"),
            "warn text level should appear in log file"
        );
        assert!(
            contents.contains(&marker),
            "warn message should appear in log file"
        );
    }

    #[test]
    fn error_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("error-marker-{}", std::process::id());
        log.error(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains("[error]"),
            "error text level should appear in log file"
        );
        assert!(
            contents.contains(&marker),
            "error message should appear in log file"
        );
    }

    #[test]
    fn stage_written_to_file_with_arrow() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("stage-marker-{}", std::process::id());
        log.stage(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains("==>"),
            "stage arrow should appear in log file"
        );
        assert!(
            contents.contains(&marker),
            "stage message should appear in log file"
        );
    }

    #[test]
    fn dry_run_written_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("dryrun-marker-{}", std::process::id());
        log.dry_run(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&marker),
            "dry run message should appear in log file: {contents}"
        );
    }

    #[test]
    fn summary_omits_log_paths() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("summary-test", TaskStatus::Ok, None);
        log.print_summary();
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            !contents.contains("log: "),
            "summary should not repeat the main log path: {contents}"
        );
        assert!(
            !contents.contains("diagnostic log: "),
            "summary should not repeat the diagnostic log path: {contents}"
        );
        assert!(
            !contents.contains("Summary"),
            "file summary should not include the full task breakdown: {contents}"
        );
        assert!(
            !contents.contains("summary-test"),
            "file summary should not repeat individual task names: {contents}"
        );
        assert!(
            contents.contains("Complete"),
            "file summary should include the final completion line: {contents}"
        );
        let lower_contents = contents.to_lowercase();
        assert!(
            lower_contents.contains("0 passed") && lower_contents.contains("1 not run"),
            "file summary should include final counts: {contents}"
        );
    }
}
