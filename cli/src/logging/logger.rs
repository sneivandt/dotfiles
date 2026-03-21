//! Structured logger with dry-run awareness and summary collection.
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::diagnostic::{DiagEvent, DiagnosticLog};
#[cfg(test)]
use super::types::Log;
use super::types::{Output, TaskEntry, TaskRecorder, TaskStatus};
use super::utils::{dotfiles_cache_dir, log_file_path, terminal_columns};
#[cfg(test)]
use super::utils::{dotfiles_cache_subdir, log_file_path_in};
use crate::tasks::TaskPhase;

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
    /// Instant when the logger was created, used for elapsed time in summary.
    start: Instant,
    /// Whether verbose output is enabled (show all stage headers and info).
    verbose: bool,
}

#[allow(clippy::print_stdout, clippy::print_stderr)]
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
            diagnostic: dotfiles_cache_dir()
                .and_then(|dir| DiagnosticLog::new(command, &dir, start)),
            start,
            verbose: true,
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
            tasks: Mutex::new(Vec::new()),
            log_file: log_file_path_in(command, cache_dir),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: Mutex::new(0),
            diagnostic: dotfiles_cache_subdir(cache_dir)
                .and_then(|dir| DiagnosticLog::new(command, &dir, start)),
            start,
            verbose: true,
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
        /// Log a phase header (major execution phase marker).
        phase, Stage, tracing::info, target: "dotfiles::phase"
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
    pub fn record_task(
        &self,
        name: &str,
        phase: TaskPhase,
        status: TaskStatus,
        message: Option<&str>,
    ) {
        if let Ok(mut guard) = self.tasks.lock() {
            guard.push(TaskEntry {
                name: name.to_string(),
                phase,
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

    /// Print the summary of all recorded tasks, grouped by phase.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            return;
        }

        let mut ok = 0u32;
        let mut not_applicable = 0u32;
        let mut skipped = 0u32;
        let mut dry_run = 0u32;
        let mut failed = 0u32;

        for task in &tasks {
            match task.status {
                TaskStatus::Ok => ok += 1,
                TaskStatus::NotApplicable => not_applicable += 1,
                TaskStatus::Skipped => skipped += 1,
                TaskStatus::DryRun => dry_run += 1,
                TaskStatus::Failed => failed += 1,
            }
        }

        // In verbose mode, show the full per-task breakdown.
        if self.verbose {
            println!();
            self.phase("Summary");

            let phases = [
                TaskPhase::Bootstrap,
                TaskPhase::Repository,
                TaskPhase::Apply,
            ];
            for phase in &phases {
                let phase_tasks: Vec<&TaskEntry> =
                    tasks.iter().filter(|t| t.phase == *phase).collect();
                let has_visible = phase_tasks
                    .iter()
                    .any(|t| t.status != TaskStatus::NotApplicable);
                if !has_visible {
                    continue;
                }
                self.info(&format!("\x1b[1m{phase}\x1b[0m"));
                for task in &phase_tasks {
                    let (icon, color) = match task.status {
                        TaskStatus::NotApplicable => continue,
                        TaskStatus::Ok => ("\u{2713}", "\x1b[32m"),
                        TaskStatus::Skipped => ("\u{25cb}", "\x1b[33m"),
                        TaskStatus::DryRun => ("~", "\x1b[35m"),
                        TaskStatus::Failed => ("\u{2717}", "\x1b[31m"),
                    };

                    let suffix = task
                        .message
                        .as_ref()
                        .map_or_else(String::new, |msg| format!(" ({msg})"));

                    self.info(&format!("{color}  {icon} {}{suffix}\x1b[0m", task.name));
                }
            }
        }

        self.always("");
        let active = ok + skipped + dry_run + failed;
        let mut parts: Vec<String> = vec![format!("\x1b[32m{ok} ok\x1b[0m")];
        if skipped > 0 {
            parts.push(format!("\x1b[33m{skipped} skipped\x1b[0m"));
        }
        if dry_run > 0 {
            parts.push(format!("\x1b[35m{dry_run} dry-run\x1b[0m"));
        }
        if failed > 0 {
            parts.push(format!("\x1b[31m{failed} failed\x1b[0m"));
        }

        let na_suffix = if not_applicable > 0 {
            format!(" \x1b[2m({not_applicable} not applicable)\x1b[0m")
        } else {
            String::new()
        };

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        self.always(&format!(
            "  {active} tasks: {}{na_suffix}",
            parts.join(", "),
        ));

        self.always(&format!("  \x1b[2mcompleted in {elapsed_str}\x1b[0m"));
        if let Some(path) = &self.log_file {
            self.always(&format!("  \x1b[2mlog: {}\x1b[0m", path.display()));
        }
    }

    /// Erase the in-progress status line from the console.
    ///
    /// No-op if no progress line is currently shown.
    /// Must be called while holding `flush_lock`.
    pub(super) fn clear_progress(&self) {
        let mut guard = self.progress_rows.lock().unwrap_or_else(|e| {
            eprintln!("warning: progress_rows lock was poisoned, recovering");
            e.into_inner()
        });
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
        let mut guard = self.progress_rows.lock().unwrap_or_else(|e| {
            eprintln!("warning: progress_rows lock was poisoned, recovering");
            e.into_inner()
        });
        *guard = 1;
    }

    /// Record that a parallel task has started.
    ///
    /// Acquires the flush lock, erases any previous progress line, adds the
    /// task to the active set, and redraws the status line.
    pub fn notify_task_start(&self, name: &str) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.clear_progress();
        let names = self.active_tasks.lock().map_or_else(
            |_| name.to_string(),
            |mut active| {
                active.push(name.to_string());
                if self.verbose {
                    active.join(", ")
                } else {
                    format!("{} tasks running\u{2026}", active.len())
                }
            },
        );
        self.draw_progress(&names);
    }

    /// Record that a parallel task has completed (successfully or otherwise).
    ///
    /// Acquires the flush lock, removes `name` from the active set, and
    /// redraws the progress line with the remaining tasks.  If no tasks
    /// remain active the progress line is cleared and `progress_rows` is
    /// set to `0`.
    ///
    /// Must be called while **not** already holding `flush_lock` to avoid
    /// deadlocking.
    pub fn notify_task_done(&self, name: &str) {
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        self.clear_progress();
        let remaining = self.active_tasks.lock().ok().and_then(|mut active| {
            active.retain(|n| n != name);
            if active.is_empty() {
                None
            } else if self.verbose {
                Some(active.join(", "))
            } else {
                Some(format!("{} tasks running\u{2026}", active.len()))
            }
        });
        if let Some(names) = remaining {
            self.draw_progress(&names);
        }
    }
}

impl Output for Logger {
    forward_log_methods!(
        stage,
        info,
        debug,
        warn,
        error,
        dry_run,
        always,
        task_result
    );

    fn is_verbose(&self) -> bool {
        self.verbose
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.diagnostic.as_ref()
    }
}

impl TaskRecorder for Logger {
    fn record_task(&self, name: &str, phase: TaskPhase, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, phase, status, message);
    }
}

/// Format a duration as a human-readable string (e.g., "1.2s", "2m 5s").
fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let mins = secs / 60;
        let remaining = secs % 60;
        format!("{mins}m {remaining}s")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::logging::isolated_logger;
    use crate::tasks::TaskPhase;
    use std::fs;

    #[test]
    fn logger_new() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(log.task_entries().is_empty(), "expected empty task list");
    }

    #[test]
    fn record_task_ok() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("symlinks", TaskPhase::Apply, TaskStatus::Ok, None);
        let tasks = log.task_entries();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
    }

    #[test]
    fn record_task_with_message() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task(
            "packages",
            TaskPhase::Apply,
            TaskStatus::Skipped,
            Some("not on arch"),
        );
        assert_eq!(
            log.task_entries()[0].message,
            Some("not on arch".to_string())
        );
    }

    #[test]
    fn record_multiple_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("a", TaskPhase::Apply, TaskStatus::Ok, None);
        log.record_task("b", TaskPhase::Apply, TaskStatus::Failed, Some("error"));
        log.record_task("c", TaskPhase::Apply, TaskStatus::DryRun, None);
        assert_eq!(log.task_entries().len(), 3);
    }

    #[test]
    fn has_failures_detects_failed_task() {
        let (log, _tmp, _guard) = isolated_logger();
        assert!(!log.has_failures());
        log.record_task("a", TaskPhase::Apply, TaskStatus::Ok, None);
        assert!(!log.has_failures());
        log.record_task("b", TaskPhase::Apply, TaskStatus::Failed, Some("error"));
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
        log.record_task("a", TaskPhase::Apply, TaskStatus::Ok, None);
        log.record_task("b", TaskPhase::Apply, TaskStatus::Failed, Some("error 1"));
        log.record_task("c", TaskPhase::Apply, TaskStatus::Failed, Some("error 2"));
        log.record_task("d", TaskPhase::Apply, TaskStatus::Skipped, None);
        assert_eq!(log.failure_count(), 2);
    }

    #[test]
    fn log_trait_delegates_to_logger() {
        let (log, _tmp, _guard) = isolated_logger();
        let log_ref: &dyn Log = &log;
        log_ref.record_task("via-trait", TaskPhase::Apply, TaskStatus::Ok, None);
        assert_eq!(log.task_entries().len(), 1);
    }

    #[test]
    fn progress_rows_zero_initially() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0);
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
            contents.contains(&marker),
            "dry run message should appear in log file: {contents}"
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

    #[test]
    fn notify_task_start_sets_progress_rows_to_one() {
        let (log, _tmp, _guard) = isolated_logger();
        assert_eq!(log.progress_rows_count(), 0, "progress_rows starts at 0");
        log.notify_task_start("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after first notify_task_start"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_done_removes_from_active_tasks() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("my-task");
        log.notify_task_done("my-task");
        let active = log.active_tasks.lock().unwrap();
        assert!(
            !active.contains(&"my-task".to_string()),
            "active_tasks should not contain 'my-task' after notify_task_done"
        );
    }

    #[test]
    fn notify_task_done_clears_progress_when_last_task_completes() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should be 1 after start"
        );
        log.notify_task_done("task-a");
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be 0 after last task completes"
        );
    }

    #[test]
    fn notify_task_done_keeps_progress_when_tasks_remain() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        log.notify_task_done("task-a");
        assert_eq!(
            log.progress_rows_count(),
            1,
            "progress_rows should still be 1 when task-b is still active"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn notify_task_done_multiple_tasks_all_complete() {
        let (log, _tmp, _guard) = isolated_logger();
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        log.notify_task_done("task-a");
        {
            let active = log.active_tasks.lock().unwrap();
            assert!(
                !active.contains(&"task-a".to_string()),
                "task-a should be removed"
            );
            assert!(
                active.contains(&"task-b".to_string()),
                "task-b should still be present"
            );
        }
        log.notify_task_done("task-b");
        {
            let active = log.active_tasks.lock().unwrap();
            assert!(
                active.is_empty(),
                "active_tasks should be empty after both tasks complete"
            );
        }
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be 0 after all tasks complete"
        );
    }

    // -------------------------------------------------------------------
    // format_elapsed
    // -------------------------------------------------------------------

    #[test]
    fn format_elapsed_sub_second() {
        let d = Duration::from_millis(450);
        assert_eq!(format_elapsed(d), "0.5s");
    }

    #[test]
    fn format_elapsed_seconds() {
        let d = Duration::from_secs_f64(3.7);
        assert_eq!(format_elapsed(d), "3.7s");
    }

    #[test]
    fn format_elapsed_minutes() {
        let d = Duration::from_secs(125);
        assert_eq!(format_elapsed(d), "2m 5s");
    }
}
