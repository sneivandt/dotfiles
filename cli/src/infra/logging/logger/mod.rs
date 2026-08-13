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

use super::runlog::RunLog;
use super::style::{TextStyle, stdout_style};
use super::types::{
    ActionCounts, LogEvent, MsgKind, Output, OutputExt as _, TaskEntry, TaskRecorder, TaskStatus,
    emit_console_event,
};
#[cfg(test)]
use super::utils::dotfiles_log_subdir;
use crate::infra::logging::TaskVisibility;

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
    /// Whether console task rows use compact status glyphs.
    pub(super) symbols: bool,
    /// Whether the current command is previewing changes without applying them.
    pub(super) dry_run: bool,
    /// Whether the separator after startup metadata has been emitted.
    startup_separator_emitted: AtomicBool,
}

/// Buffered user-facing detail lines emitted by a completed task.
#[derive(Debug, Clone)]
pub(in crate::infra::logging) struct TaskDetailEntry {
    /// Scheduler identity key for the owning task, when available.
    pub(super) task_id: Option<String>,
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
        let run_log = RunLog::create(command, start).map(Arc::new);
        if run_log.is_none() {
            super::runlog::warn_degraded("the log directory or file could not be created");
        }
        Self::build(command, run_log, start)
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
        if run_log.is_none() {
            super::runlog::warn_degraded("the log directory or file could not be created");
        }
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
            task_total: AtomicUsize::new(0),
            tasks_completed: AtomicUsize::new(0),
            tasks_excluded: AtomicUsize::new(0),
            run_log,
            start,
            verbose: true,
            symbols: true,
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

    /// Set whether console task rows use compact status glyphs.
    pub const fn set_symbols(&mut self, symbols: bool) {
        self.symbols = symbols;
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

    /// Acquire the recorded-task list, recovering from poisoning.
    ///
    /// Task panics are expected and survivable — the scheduler catches them so
    /// the rest of the run continues — which means this lock can genuinely end
    /// up poisoned. Treating that as "no tasks" silently empties the run
    /// summary and the progress counter, turning one task's panic into a run
    /// that appears to have done nothing. The entries are an append-only log
    /// with no invariant spanning them, so recovering the guard keeps every
    /// task recorded before and after the panic.
    pub(in crate::infra::logging) fn lock_tasks(
        &self,
    ) -> std::sync::MutexGuard<'_, Vec<TaskEntry>> {
        self.tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Acquire the per-task detail lines, recovering from poisoning.
    ///
    /// Same reasoning as [`Logger::lock_tasks`].
    pub(in crate::infra::logging) fn lock_task_details(
        &self,
    ) -> std::sync::MutexGuard<'_, Vec<TaskDetailEntry>> {
        self.task_details
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Acquire the active-task list, recovering from poisoning.
    ///
    /// Same reasoning as [`Logger::lock_tasks`].
    pub(in crate::infra::logging) fn lock_active_tasks(
        &self,
    ) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.active_tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Return a handle to the run log, if available.
    pub(in crate::infra::logging) fn run_log_handle(&self) -> Option<Arc<RunLog>> {
        self.run_log.clone()
    }

    /// Whether persistent run logging is available and remains writable.
    #[must_use]
    pub fn run_log_is_healthy(&self) -> bool {
        self.run_log.as_deref().is_some_and(RunLog::is_healthy)
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
        self.lock_tasks().clone()
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
        self.record_task_with_metadata(name, status, message, actions, TaskVisibility::Visible);
    }

    /// Record a task result with structured action totals and presentation metadata.
    pub fn record_task_with_metadata(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visibility: TaskVisibility,
    ) {
        self.lock_tasks().push(TaskEntry {
            task_id: None,
            name: name.to_string(),
            status,
            message: message.map(String::from),
            actions,
            visibility,
            duration: None,
        });
    }

    /// Record an engine task by scheduler identity.
    pub fn record_task_with_identity(
        &self,
        task_id: &str,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visibility: TaskVisibility,
    ) {
        self.lock_tasks().push(TaskEntry {
            task_id: Some(task_id.to_string()),
            name: name.to_string(),
            status,
            message: message.map(String::from),
            actions,
            visibility,
            duration: None,
        });
    }

    /// Attach a measured run duration to the most recent entry for `name`.
    pub fn record_task_duration(&self, name: &str, duration: std::time::Duration) {
        self.record_task_duration_by(|task| task.name == name, duration);
    }

    /// Attach a measured duration to the entry for a scheduler identity.
    pub fn record_task_duration_by_id(&self, task_id: &str, duration: std::time::Duration) {
        self.record_task_duration_by(|task| task.task_id.as_deref() == Some(task_id), duration);
    }

    fn record_task_duration_by(
        &self,
        matches: impl Fn(&TaskEntry) -> bool,
        duration: std::time::Duration,
    ) {
        if let Some(task) = self
            .lock_tasks()
            .iter_mut()
            .rev()
            .find(|task| matches(task))
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
        self.mark_task_completed_by(|task| task.name == task_name);
    }

    /// Record completion using the task's scheduler identity.
    pub fn mark_task_completed_by_id(&self, task_id: &str) {
        self.mark_task_completed_by(|task| task.task_id.as_deref() == Some(task_id));
    }

    fn mark_task_completed_by(&self, matches: impl Fn(&TaskEntry) -> bool) {
        let task = self
            .lock_tasks()
            .iter()
            .rev()
            .find(|task| matches(task))
            .cloned();
        let Some(task) = task else {
            return;
        };
        if !task.visibility.is_visible() {
            return;
        }
        if task.status == TaskStatus::NotApplicable {
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

    pub(in crate::infra::logging) fn task_is_visible(&self, name: &str) -> bool {
        self.task_is_visible_by(|task| task.name == name)
    }

    pub(in crate::infra::logging) fn task_is_visible_by_id(&self, task_id: &str) -> bool {
        self.task_is_visible_by(|task| task.task_id.as_deref() == Some(task_id))
    }

    fn task_is_visible_by(&self, matches: impl Fn(&TaskEntry) -> bool) -> bool {
        self.lock_tasks()
            .iter()
            .rev()
            .find(|task| matches(task))
            .is_none_or(|task| task.visibility.is_visible())
    }

    /// Return the recorded outcome message for the most recent entry named `name`.
    pub(in crate::infra::logging) fn recorded_task_message(&self, name: &str) -> Option<String> {
        self.recorded_task_message_by(|task| task.name == name)
    }

    pub(in crate::infra::logging) fn recorded_task_message_by_id(
        &self,
        task_id: &str,
    ) -> Option<String> {
        self.recorded_task_message_by(|task| task.task_id.as_deref() == Some(task_id))
    }

    fn recorded_task_message_by(&self, matches: impl Fn(&TaskEntry) -> bool) -> Option<String> {
        self.lock_tasks()
            .iter()
            .rev()
            .find(|task| matches(task))
            .and_then(|task| task.message.clone())
    }

    /// Record buffered user-facing detail lines for a completed task.
    pub(in crate::infra::logging) fn record_task_details(&self, name: &str, lines: Vec<String>) {
        self.record_task_details_for(None, name, lines);
    }

    /// Record buffered detail lines for an engine task identity.
    pub(in crate::infra::logging) fn record_task_details_by_id(
        &self,
        task_id: &str,
        name: &str,
        lines: Vec<String>,
    ) {
        self.record_task_details_for(Some(task_id), name, lines);
    }

    fn record_task_details_for(&self, task_id: Option<&str>, name: &str, lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        self.lock_task_details().push(TaskDetailEntry {
            task_id: task_id.map(str::to_string),
            name: name.to_string(),
            lines,
        });
    }

    /// Count the number of failed tasks.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.lock_tasks()
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .count()
    }

    /// Emit the single blank line that separates startup metadata from details.
    ///
    /// The startup header calls this immediately, so the header always stands
    /// apart even when the run produces no task output. The remaining callers
    /// are idempotent guards for paths that emit before — or without — a header.
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
        emit_console_event!(kind, &*msg);
    }

    fn run_log(&self) -> Option<&RunLog> {
        self.run_log.as_deref()
    }

    fn status_line(&self, msg: &str) {
        if !stdout_supports_progress() {
            return;
        }
        let _guard = self.lock_flush();
        self.replace_status_line(&stdout_style().paint(TextStyle::Dim, msg));
    }

    fn clear_status_line(&self) {
        if stdout_supports_progress() {
            self.clear_status();
        }
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
        visibility: TaskVisibility,
    ) {
        self.record_task_with_metadata(name, status, message, actions, visibility);
    }

    fn record_task_with_identity(
        &self,
        task_id: &str,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visibility: TaskVisibility,
    ) {
        self.record_task_with_identity(task_id, name, status, message, actions, visibility);
    }

    fn record_task_duration(&self, name: &str, duration: std::time::Duration) {
        self.record_task_duration(name, duration);
    }

    fn record_task_duration_by_id(
        &self,
        task_id: &str,
        _name: &str,
        duration: std::time::Duration,
    ) {
        self.record_task_duration_by_id(task_id, duration);
    }
}

#[cfg(test)]
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
