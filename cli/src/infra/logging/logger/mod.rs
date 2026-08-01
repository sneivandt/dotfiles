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

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU16;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::runlog::{LogEvent, RunLog};
use super::types::{
    ActionCounts, MsgKind, Output, OutputExt as _, TaskEntry, TaskRecorder, TaskStatus,
    emit_console_event,
};
#[cfg(test)]
use super::utils::dotfiles_log_subdir;

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent per-run log file in the
/// dotfiles log directory (`%LOCALAPPDATA%\dotfiles\logs` on Windows,
/// `$XDG_STATE_HOME/dotfiles/logs` elsewhere) with timestamps and ANSI codes
/// stripped, regardless of the verbose flag.
#[derive(Debug)]
pub struct Logger {
    /// Command currently being executed (`install`, `update`, etc.).
    pub(super) command: String,
    pub(super) tasks: Mutex<Vec<TaskEntry>>,
    pub(super) task_details: Mutex<Vec<TaskDetailEntry>>,
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
    /// Whether the most recent task block printed indented detail lines.
    ///
    /// Drives the blank line that separates a detail block from whatever row
    /// follows it, so a long list of actions never runs straight into the next
    /// task's status row.
    pub(super) last_block_had_details: AtomicBool,
    /// Number of tasks scheduled for this run, used as the progress denominator.
    pub(super) task_total: AtomicUsize,
    /// Number of tasks that have finished, used as the progress numerator.
    pub(super) tasks_completed: AtomicUsize,
    /// Number of scheduled tasks the run does not report on, subtracted from
    /// the progress denominator once each one finishes.
    pub(super) tasks_excluded: AtomicUsize,
    /// The run log; `None` when the log directory is unavailable.
    ///
    /// Shared with the tracing bridge so that logger messages and raw
    /// `tracing` events land in the same file.
    pub(super) run_log: Option<Arc<RunLog>>,
    /// Instant when the logger was created, used for elapsed time in summary.
    pub(super) start: Instant,
    /// Whether verbose output is enabled (show applicable task statuses and details).
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
    /// Opens the run log for `command` in the dotfiles log directory.  If
    /// the file cannot be created the run continues without a file log.
    #[must_use]
    pub fn new(command: &str) -> Self {
        let start = Instant::now();
        Self::build(command, RunLog::create(command, start).map(Arc::new), start)
    }

    /// Create a new logger using an explicit base directory.
    ///
    /// Like [`new`](Self::new) but resolves the run log under `base_dir`
    /// instead of reading the log directory from the environment.  Intended
    /// for tests that need an isolated logger without mutating process-global
    /// state.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_in(command: &str, base_dir: &std::path::Path) -> Self {
        let start = Instant::now();
        let run_log = dotfiles_log_subdir(base_dir)
            .and_then(|dir| RunLog::new(command, &dir, start))
            .map(Arc::new);
        Self::build(command, run_log, start)
    }

    fn build(command: &str, run_log: Option<Arc<RunLog>>, start: Instant) -> Self {
        Self {
            command: command.to_string(),
            tasks: Mutex::new(Vec::new()),
            task_details: Mutex::new(Vec::new()),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            progress_rows: AtomicU16::new(0),
            status_row_visible: AtomicBool::new(false),
            task_console_output_emitted: AtomicBool::new(false),
            last_block_had_details: AtomicBool::new(false),
            task_total: AtomicUsize::new(0),
            tasks_completed: AtomicUsize::new(0),
            tasks_excluded: AtomicUsize::new(0),
            run_log,
            start,
            verbose: true,
            dry_run: false,
            startup_separator_emitted: AtomicBool::new(false),
        }
    }

    /// Set the verbose mode on this logger.
    ///
    /// Also updates the global [`subscriber`](super::subscriber) flag so the
    /// console formatter stays in sync.
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
        super::subscriber::set_verbose(verbose);
    }

    /// Set dry-run mode on this logger for summary rendering.
    pub const fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
    }

    /// Acquire the console flush lock, recovering from poisoning.
    ///
    /// A panicking task poisons this lock, but the mutex guards console
    /// cursor sequencing rather than shared data, so there is no invariant to
    /// restore: recovering keeps the remaining tasks and the final summary
    /// visible instead of turning one panic into a cascade of them.
    #[allow(
        clippy::print_stderr,
        reason = "the console is the only sink able to report that console sequencing degraded"
    )]
    pub(in crate::infra::logging) fn lock_flush(&self) -> std::sync::MutexGuard<'_, ()> {
        self.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        })
    }

    /// Return a handle to the run log, if available.
    pub(in crate::infra::logging) fn run_log_handle(&self) -> Option<Arc<RunLog>> {
        self.run_log.clone()
    }

    /// Return whether verbose output mode is enabled.
    pub const fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Return the sentence-case title of the current command.
    #[must_use]
    pub fn command_title(&self) -> String {
        let mut chars = self.command.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + chars.as_str()
        })
    }

    /// Return the run log file path, if available.
    #[cfg(test)]
    #[must_use]
    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.run_log.as_deref().map(RunLog::path)
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

    /// Log a compact task-result line.
    pub fn task_result(&self, msg: &str) {
        if let Some(run_log) = &self.run_log {
            run_log.emit(LogEvent::Info, msg);
        }
        tracing::info!(target: "dotfiles::ui::task_result", "{msg}");
    }

    /// Record a task result for the summary.
    pub fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task_with_actions(name, status, message, ActionCounts::default());
    }

    /// Record a task result and its structured action totals for the summary.
    pub fn record_task_with_actions(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
    ) {
        self.record_task_with_metadata(name, status, message, actions, true);
    }

    /// Record a task result with structured action totals and presentation metadata.
    pub fn record_task_with_metadata(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visible: bool,
    ) {
        if let Ok(mut guard) = self.tasks.lock() {
            guard.push(TaskEntry {
                name: name.to_string(),
                status,
                message: message.map(String::from),
                actions,
                visible,
                duration: None,
            });
        }
    }

    /// Attach a measured run duration to the most recent entry for `name`.
    pub fn record_task_duration(&self, name: &str, duration: std::time::Duration) {
        if let Ok(mut guard) = self.tasks.lock()
            && let Some(task) = guard.iter_mut().rev().find(|task| task.name == name)
        {
            task.duration = Some(duration);
        }
    }

    /// Add a scheduled batch of tasks to the progress denominator.
    ///
    /// A run may schedule more than one dependency graph — tasks discovered
    /// after a boundary run in a second graph — so batches accumulate rather
    /// than replace. Only user-visible tasks count, matching the run summary.
    /// A total of zero suppresses the counter entirely.
    pub fn add_task_total(&self, total: usize) {
        self.task_total.fetch_add(total, Ordering::Relaxed);
    }

    /// Record that one more task has finished, for the progress counter.
    ///
    /// The counter must agree with the run summary, which reports on neither
    /// internal tasks nor tasks that turned out not to apply. Internal tasks
    /// were never added to the total, so they are simply ignored. Applicability
    /// is only known once a task has run, so a non-applicable task leaves the
    /// denominator instead of advancing the numerator.
    pub fn mark_task_completed(&self, task_name: &str) {
        if !self.task_is_visible(task_name) {
            return;
        }
        if self.recorded_task_status(task_name) == Some(TaskStatus::NotApplicable) {
            self.tasks_excluded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_completed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Return the completed/total task counts for the progress line.
    ///
    /// The numerator is clamped to the total so a miscount can never render a
    /// nonsensical `22/20`.
    pub(in crate::infra::logging) fn task_progress(&self) -> Option<(usize, usize)> {
        let total = self
            .task_total
            .load(Ordering::Relaxed)
            .saturating_sub(self.tasks_excluded.load(Ordering::Relaxed));
        (total > 0).then(|| {
            let done = self.tasks_completed.load(Ordering::Relaxed).min(total);
            (done, total)
        })
    }

    /// Look up the status recorded for a task, if it has finished.
    fn recorded_task_status(&self, name: &str) -> Option<TaskStatus> {
        self.tasks.lock().map_or(None, |guard| {
            guard
                .iter()
                .rev()
                .find(|task| task.name == name)
                .map(|task| task.status)
        })
    }

    pub(in crate::infra::logging) fn task_is_visible(&self, name: &str) -> bool {
        self.tasks.lock().map_or(true, |guard| {
            guard
                .iter()
                .rev()
                .find(|task| task.name == name)
                .is_none_or(|task| task.visible)
        })
    }

    /// Return the recorded outcome message for the most recent entry named `name`.
    pub(in crate::infra::logging) fn recorded_task_message(&self, name: &str) -> Option<String> {
        self.tasks.lock().ok().and_then(|guard| {
            guard
                .iter()
                .rev()
                .find(|task| task.name == name)
                .and_then(|task| task.message.clone())
        })
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
    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
        if let Some(run_log) = &self.run_log {
            run_log.emit(kind.log_event(), &msg);
        }
        if self.verbose && matches!(kind, MsgKind::Debug) {
            // Verbose prints debug lines indented, so the startup diagnostics
            // form a detail block that the first task row must be spaced from.
            self.last_block_had_details.store(true, Ordering::Relaxed);
        }
        emit_console_event!(kind, &*msg);
    }

    fn run_log(&self) -> Option<&RunLog> {
        self.run_log.as_deref()
    }
}

impl TaskRecorder for Logger {
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.record_task(name, status, message);
    }

    fn record_task_with_actions(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
    ) {
        self.record_task_with_actions(name, status, message, actions);
    }

    fn record_task_with_metadata(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visible: bool,
    ) {
        self.record_task_with_metadata(name, status, message, actions, visible);
    }

    fn record_task_duration(&self, name: &str, duration: std::time::Duration) {
        self.record_task_duration(name, duration);
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
    fn command_titles_are_sentence_case() {
        let cache = tempfile::tempdir().expect("temporary cache should be created");
        for (command, expected) in [
            ("install", "Install"),
            ("update", "Update"),
            ("uninstall", "Uninstall"),
            ("test", "Test"),
        ] {
            let log = Logger::new_in(command, cache.path());
            assert_eq!(log.command_title(), expected);
        }
    }

    #[test]
    fn record_task_ok() {
        let (log, _tmp, _guard) = isolated_logger();
        log.record_task("symlinks", TaskStatus::Ok, None);
        let tasks = log.task_entries();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "symlinks");
        assert_eq!(tasks[0].status, TaskStatus::Ok);
        assert_eq!(tasks[0].actions, ActionCounts::default());
    }

    #[test]
    fn record_task_with_actions_stores_structured_counts() {
        let (log, _tmp, _guard) = isolated_logger();
        let actions = ActionCounts {
            applied: 4,
            planned: 0,
            skipped: 1,
            failed: 0,
        };

        log.record_task_with_actions("symlinks", TaskStatus::Changed, None, actions);

        assert_eq!(log.task_entries()[0].actions, actions);
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
    fn run_log_accessible_via_trait() {
        let (log, _tmp, _guard) = isolated_logger();
        let log_ref: &dyn Log = &log;
        assert!(
            log_ref.run_log().is_some(),
            "run_log() should be accessible via Log trait"
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
    fn stage_written_to_file_with_event_kind() {
        let (log, _tmp, _guard) = isolated_logger();
        let marker = format!("stage-marker-{}", std::process::id());
        log.stage(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&format!("[stage] {marker}")),
            "stage should be recorded under the stage event kind: {contents}"
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
            !contents.contains("run log: "),
            "summary should not repeat the run log path: {contents}"
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
            contents.contains("No checks ran ·"),
            "file summary should include the final completion line: {contents}"
        );
        let lower_contents = contents.to_lowercase();
        assert!(
            !lower_contents.contains("not run") && !lower_contents.contains("checks:"),
            "test summary should omit unchanged checks: {contents}"
        );
    }
}
