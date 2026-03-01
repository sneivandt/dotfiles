//! Structured logger with dry-run awareness and summary collection.
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use super::diagnostic::{DiagEvent, DiagnosticLog};
use super::types::{Log, TaskEntry, TaskStatus};
use super::utils::{log_file_path, terminal_columns};

/// Implement the display methods of [`Log`] by delegating to inherent methods
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
    tasks: Mutex<Vec<TaskEntry>>,
    log_file: Option<PathBuf>,
    /// Serializes console output from parallel task flushes.
    pub(super) flush_lock: Mutex<()>,
    /// Names of tasks currently executing in parallel.
    pub(super) active_tasks: Mutex<Vec<String>>,
    /// Whether a progress line is currently displayed (`0` = no, `1` = yes).
    ///
    /// The progress line is always truncated to fit within a single terminal
    /// row, so the only valid values are `0` and `1`.  This avoids multi-row
    /// cursor arithmetic that can erase real output when the terminal width
    /// differs from the `COLUMNS` environment variable.
    pub(super) progress_rows: Mutex<u16>,
    /// High-precision diagnostic log; `None` when the cache dir is unavailable.
    pub(super) diagnostic: Option<DiagnosticLog>,
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
            tasks: Mutex::new(Vec::new()),
            log_file: log_file_path(command),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: Mutex::new(0),
            diagnostic: DiagnosticLog::new(command, start),
        }
    }

    /// Return the diagnostic log, if available.
    pub const fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }

    /// Return the log file path, if available.
    #[cfg(test)]
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
        *self
            .progress_rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Log an error message.
    pub fn error(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Error, msg);
        }
        tracing::error!("{msg}");
    }

    /// Log a warning message.
    pub fn warn(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Warn, msg);
        }
        tracing::warn!("{msg}");
    }

    /// Log a stage header (major section).
    pub fn stage(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Stage, msg);
        }
        tracing::info!(target: "dotfiles::stage", "{msg}");
    }

    /// Log an informational message.
    pub fn info(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Info, msg);
        }
        tracing::info!("{msg}");
    }

    /// Log a debug message (suppressed on console unless verbose; always
    /// written to the log file via the [`FileLayer`](super::subscriber::FileLayer)).
    pub fn debug(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::Debug, msg);
        }
        tracing::debug!("{msg}");
    }

    /// Log a dry-run action message.
    pub fn dry_run(&self, msg: &str) {
        if let Some(d) = &self.diagnostic {
            d.emit(DiagEvent::DryRun, msg);
        }
        tracing::info!(target: "dotfiles::dry_run", "{msg}");
    }

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

    /// Return `true` if any recorded task has failed.
    #[must_use]
    #[allow(dead_code)]
    pub fn has_failures(&self) -> bool {
        self.failure_count() > 0
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

    /// Print the summary of all recorded tasks.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            return;
        }

        println!();
        self.stage("Summary");

        let mut ok = 0u32;
        let mut not_applicable = 0u32;
        let mut skipped = 0u32;
        let mut dry_run = 0u32;
        let mut failed = 0u32;

        for task in &tasks {
            let (icon, color) = match task.status {
                TaskStatus::Ok => {
                    ok += 1;
                    ("✓", "\x1b[32m")
                }
                TaskStatus::NotApplicable => {
                    not_applicable += 1;
                    ("·", "\x1b[2m")
                }
                TaskStatus::Skipped => {
                    skipped += 1;
                    ("○", "\x1b[33m")
                }
                TaskStatus::DryRun => {
                    dry_run += 1;
                    ("~", "\x1b[37m")
                }
                TaskStatus::Failed => {
                    failed += 1;
                    ("✗", "\x1b[31m")
                }
            };

            let suffix = task
                .message
                .as_ref()
                .map_or_else(String::new, |msg| format!(" ({msg})"));

            self.info(&format!("{color}{icon} {}{suffix}\x1b[0m", task.name));
        }

        println!();
        let total = ok + not_applicable + skipped + dry_run + failed;
        self.info(&format!(
            "{total} tasks: \x1b[32m{ok} ok\x1b[0m, \x1b[2m{not_applicable} n/a\x1b[0m, \x1b[33m{skipped} skipped\x1b[0m, \x1b[37m{dry_run} dry-run\x1b[0m, \x1b[31m{failed} failed\x1b[0m"
        ));

        if let Some(path) = &self.log_file {
            self.info(&format!("\x1b[2mlog: {}\x1b[0m", path.display()));
        }
    }

    /// Erase the in-progress status line from the console.
    ///
    /// No-op if no progress line is currently shown.
    /// Must be called while holding `flush_lock`.
    pub(super) fn clear_progress(&self) {
        let mut guard = self
            .progress_rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard > 0 {
            print!("\r\x1b[K");
            std::io::stdout().flush().ok();
            *guard = 0;
        }
    }

    /// Print an in-progress status line to the console and mark it as shown.
    ///
    /// The task-name list is truncated to fit within a single terminal row so
    /// that [`clear_progress`](Self::clear_progress) never needs cursor-up
    /// movement (which is fragile when the terminal width is unknown).
    ///
    /// Must be called while holding `flush_lock`.
    pub(super) fn draw_progress(&self, names: &str) {
        let cols = terminal_columns();
        let prefix_width = 4;
        let max_name_chars = cols.saturating_sub(prefix_width);
        let display_names = if names.chars().count() > max_name_chars {
            let truncated: String = names
                .chars()
                .take(max_name_chars.saturating_sub(1))
                .collect();
            format!("{truncated}…")
        } else {
            names.to_string()
        };
        print!("  \x1b[2m▹ {display_names}\x1b[0m");
        std::io::stdout().flush().ok();
        let mut guard = self
            .progress_rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = 1;
    }

    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        let _guard = self
            .flush_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_progress();
        let names = self.active_tasks.lock().map_or_else(
            |_| name.to_string(),
            |mut active| {
                active.push(name.to_string());
                active.join(", ")
            },
        );
        self.draw_progress(&names);
    }
}

impl Log for Logger {
    forward_log_methods!(stage, info, debug, warn, error, dry_run);

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, status, message);
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::logging::isolated_logger;
    use std::fs;
    use std::sync::Arc;

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
    fn has_failures_detects_failed_task() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(!log.has_failures());
        log.record_task("a", TaskStatus::Ok, None);
        assert!(!log.has_failures());
        log.record_task("b", TaskStatus::Failed, Some("error"));
        assert!(log.has_failures());
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
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
    }

    #[test]
    fn notify_task_start_sets_progress_rows() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("update");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after notify_task_start"
        );
    }

    #[test]
    fn draw_progress_caps_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        let long_names = "a".repeat(500);
        log.draw_progress(&long_names);
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should always be 1 even for very long names"
        );
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
            "warn tag should appear in log file"
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
            "error tag should appear in log file"
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
            contents.contains("[dry run]"),
            "dry run tag should appear in log file"
        );
        assert!(
            contents.contains(&marker),
            "dry run message should appear in log file"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_start_adds_to_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("my-task");
        let active = log.active_tasks.lock().unwrap();
        assert!(
            active.contains(&"my-task".to_string()),
            "active_tasks should contain 'my-task'"
        );
    }
}
