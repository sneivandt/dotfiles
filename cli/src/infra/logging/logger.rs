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

#[cfg(test)]
mod isolation_tests {
    use super::Logger;

    #[test]
    #[should_panic(expected = "unit tests must use Logger::test")]
    fn production_constructor_is_blocked_before_resolving_host_logs() {
        drop(Logger::new("test"));
    }

    #[test]
    fn ordinary_test_logger_has_no_durable_sink() {
        assert!(Logger::test().run_log.is_none());
    }
}

use std::borrow::Cow;
use std::io::IsTerminal as _;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::runlog::RunLog;
use super::types::{LogEvent, MsgKind, Output, TaskEntry, TaskRecorder, TaskStatus};
use super::utils::dotfiles_log_subdir;

/// Structured logger with dry-run awareness and summary collection.
///
/// All messages are always written to a persistent per-run log file in the
/// dotfiles log directory (`%LOCALAPPDATA%\dotfiles\logs` on Windows,
/// `$XDG_STATE_HOME/dotfiles/logs` elsewhere) with timestamps and ANSI codes
/// stripped, regardless of the verbose flag.
#[derive(Debug)]
pub struct Logger {
    pub(in crate::infra::logging) console: Arc<super::console::Console>,
    /// Command currently being executed (`install`, `update`, etc.).
    pub(super) command: String,
    pub(super) tasks: Mutex<Vec<TaskEntry>>,
    /// Serializes console output from parallel task flushes.
    pub(super) flush_lock: Mutex<()>,
    /// Names of tasks currently executing in parallel.
    pub(super) active_tasks: Mutex<Vec<String>>,
    /// Number of tasks scheduled for this run, used as the progress denominator.
    pub(super) task_total: AtomicUsize,
    /// Animation frame, independent of task completion counts.
    pub(super) progress_frame: AtomicUsize,
    /// Number of tasks that have finished, used as the progress numerator.
    pub(super) tasks_completed: AtomicUsize,
    /// The run log; `None` when the log directory is unavailable.
    ///
    /// Shared with the tracing bridge so that logger messages and raw
    /// `tracing` events land in the same file.
    pub(super) run_log: Option<Arc<RunLog>>,
    /// Instant when the logger was created, used for elapsed time in summary.
    pub(super) start: Instant,
    invocation_started: SystemTime,
    elapsed_before_start: Duration,
    startup_transition: Mutex<Option<String>>,
    pub(super) progress_terminal: bool,
    /// Whether console task rows use compact status glyphs.
    pub(super) symbols: bool,
    /// Whether the current command is previewing changes without applying them.
    pub(super) dry_run: bool,
    finished: AtomicBool,
}

impl Logger {
    /// Create a new logger.
    ///
    /// Opens the run log for `command` in the dotfiles log directory.  If
    /// the file cannot be created the run continues without a file log.
    #[must_use]
    #[expect(
        clippy::assertions_on_constants,
        clippy::missing_panics_doc,
        reason = "unit-test builds deliberately reject the production log sink before resolving host paths"
    )]
    pub fn new(command: &str) -> Self {
        assert!(
            !cfg!(test),
            "unit tests must use Logger::test or an explicitly isolated logger"
        );
        let start = Instant::now();
        let run_log = RunLog::create(command, start).map(Arc::new);
        if run_log.is_none() {
            super::runlog::warn_degraded("the log directory or file could not be created");
        }
        Self::build(command, run_log, start, true)
    }

    /// Create a silent, nonpersistent logger for unit tests.
    #[cfg(test)]
    pub(crate) fn test() -> Self {
        Self::build("test", None, Instant::now(), false)
    }

    /// Create a new logger using an explicit base directory.
    ///
    /// Like [`new`](Self::new) but resolves the run log under `base_dir`
    /// instead of reading the log directory from the environment.  Intended
    /// for tests that need an isolated logger without mutating process-global
    /// state.
    #[must_use]
    pub(crate) fn new_in(command: &str, base_dir: &std::path::Path) -> Self {
        let start = Instant::now();
        let run_log = dotfiles_log_subdir(base_dir)
            .and_then(|dir| RunLog::new(command, &dir, start))
            .map(Arc::new);
        if run_log.is_none() {
            super::runlog::warn_degraded("the log directory or file could not be created");
        }
        Self::build(command, run_log, start, false)
    }

    fn build(
        command: &str,
        run_log: Option<Arc<RunLog>>,
        start: Instant,
        console_output: bool,
    ) -> Self {
        Self {
            console: Arc::new(super::console::Console::new(console_output)),
            command: command.to_string(),
            tasks: Mutex::new(Vec::new()),
            flush_lock: Mutex::new(()),
            active_tasks: Mutex::new(Vec::new()),
            task_total: AtomicUsize::new(0),
            progress_frame: AtomicUsize::new(0),
            tasks_completed: AtomicUsize::new(0),
            run_log,
            start,
            invocation_started: SystemTime::now() - start.elapsed(),
            elapsed_before_start: Duration::ZERO,
            startup_transition: Mutex::new(None),
            progress_terminal: std::io::stdout().is_terminal(),
            symbols: true,
            dry_run: false,
            finished: AtomicBool::new(false),
        }
    }

    /// Configure console output without changing retained logs.
    pub fn set_output_mode(&mut self, mode: super::OutputMode) {
        self.console.lock().mode = mode;
    }

    /// Restore the invocation clock without changing process-local log timings.
    pub(crate) fn set_invocation_start(
        &mut self,
        started: SystemTime,
    ) -> Result<(), std::time::SystemTimeError> {
        self.elapsed_before_start = self.invocation_started.duration_since(started)?;
        self.invocation_started = started;
        Ok(())
    }

    pub(crate) const fn invocation_start(&self) -> SystemTime {
        self.invocation_started
    }

    fn invocation_elapsed(&self) -> Duration {
        self.elapsed_before_start
            .saturating_add(self.start.elapsed())
    }

    /// Keep a completed upgrade beneath the next process's startup header.
    pub(crate) fn queue_startup_transition(&self, message: String) {
        if let Some(run) = &self.run_log {
            run.emit(LogEvent::Info, &message);
        }
        *self
            .startup_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message);
    }

    pub(crate) fn startup_transition(&self) -> Option<String> {
        self.startup_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn take_startup_transition(&self) -> Option<String> {
        self.startup_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn flush_startup_transition(&self) {
        if let Some(message) = self.take_startup_transition() {
            self.emit_console(MsgKind::Transition, &message);
        }
    }

    pub(in crate::infra::logging) fn is_quiet(&self) -> bool {
        self.console.lock().mode == super::OutputMode::Quiet
    }

    /// Begin a process run even when persistent logging is unavailable.
    pub(crate) fn start_run(&self, parent_run_id: Option<String>) {
        let run_id = self.run_log.as_ref().map_or_else(
            || {
                format!(
                    "unlogged-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros()
                )
            },
            |run| run.id(),
        );
        self.console.lock().run_id = Some(run_id.clone());
        self.console.record(
            &super::log_thread_name(),
            super::records::Record::RunStart {
                run_id,
                command: self.command.clone(),
                parent_run_id: parent_run_id.clone(),
            },
        );
        if let Some(run) = &self.run_log {
            run.start_run(&self.command, parent_run_id);
        }
    }

    /// Write a fact to the independent console and persistent sinks.
    pub(crate) fn record(&self, record: super::records::Record) {
        self.record_in_context(&super::log_thread_name(), record);
    }

    fn record_in_context(&self, context: &str, record: super::records::Record) {
        self.console.record(context, record.clone());
        if let Some(run) = &self.run_log {
            run.record_in_context(context, record);
        }
    }

    /// Set verbosity for both logger messages and raw tracing diagnostics.
    pub fn set_verbose(&mut self, verbose: bool) {
        self.console.lock().verbose = verbose;
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
    #[expect(
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

    /// Whether a started task recorded an interruption, for presentation tests.
    #[cfg(test)]
    pub(crate) fn has_interrupted_tasks(&self) -> bool {
        self.lock_tasks()
            .iter()
            .any(|task| task.status == TaskStatus::Interrupted && !task.is_unstarted_interruption())
    }

    /// Return whether verbose output mode is enabled.
    pub fn is_verbose(&self) -> bool {
        self.console.lock().verbose
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
        self.console.lock().progress_rows
    }

    /// Return whether the active-task status row is currently displayed (test-only).
    #[cfg(test)]
    pub(crate) fn status_row_visible(&self) -> bool {
        self.console.lock().progress_rows > 0
    }

    /// Return whether task console output has been emitted (test-only).
    #[cfg(test)]
    pub(crate) fn task_console_output_emitted(&self) -> bool {
        self.console.lock().task_output_emitted
    }

    /// Render a message without recording it again.
    pub(in crate::infra::logging) fn emit_console(&self, kind: MsgKind, msg: &str) {
        self.console.emit(kind, msg, self.symbols);
    }

    #[cfg(test)]
    pub(crate) fn captured_lines(&self) -> Vec<String> {
        self.console.captured_lines()
    }

    #[cfg(test)]
    pub(crate) fn captured_output(&self) -> Vec<(bool, String)> {
        self.console.lock().captured.clone()
    }

    /// Record a task result for the summary.
    pub fn record_task(&self, task: TaskEntry) {
        self.record_in_context(
            &task.task_id,
            super::records::Record::TaskResult {
                task_id: task.task_id.clone(),
                selector: task.selector.clone(),
                name: task.name.clone(),
                status: task.status,
                reason: task.message.clone(),
                actions: task.actions,
            },
        );
        self.lock_tasks().push(task);
    }

    /// Attach a measured duration to the entry for a scheduler identity.
    pub fn record_task_duration(&self, task_id: &str, duration: Duration) {
        self.record_in_context(
            task_id,
            super::records::Record::TaskDuration {
                task_id: task_id.into(),
                elapsed_us: super::records::elapsed_us(duration),
            },
        );
        if let Some(task) = self
            .lock_tasks()
            .iter_mut()
            .rev()
            .find(|task| task.task_id == task_id)
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
    /// Internal tasks were never added to the total, so they are ignored.
    /// Every visible scheduled task advances the numerator, including tasks
    /// that turn out not to apply, so the denominator remains stable.
    pub fn mark_task_completed(&self, task_id: &str) {
        let visible = self
            .lock_tasks()
            .iter()
            .rev()
            .find(|task| task.task_id == task_id)
            .is_some_and(|task| task.visibility.is_visible());
        if visible {
            self.tasks_completed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Return the completed/total task counts for the progress line.
    ///
    /// The numerator is clamped to the total so a miscount can never render a
    /// nonsensical `22/20`.
    pub(in crate::infra::logging) fn task_progress(&self) -> Option<(usize, usize)> {
        let total = self.task_total.load(Ordering::Relaxed);
        (total > 0).then(|| {
            let done = self.tasks_completed.load(Ordering::Relaxed).min(total);
            (done, total)
        })
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
    /// Callers defer this until the first task block or final totals so
    /// consecutive startup lines remain a compact header. The guard is
    /// idempotent for paths that emit before — or without — a header.
    pub fn separate_from_startup(&self) {
        self.console.lock().separate_from_startup();
    }

    /// Separate a foreground interaction from surrounding console output.
    /// Reuse an existing blank line and keep presentation out of stored records.
    pub fn separate_console_block(&self) {
        self.console.lock().separate_block();
    }

    /// Let task progress use the row below the separator once no restart notice can follow.
    pub(crate) fn separate_after_transition(&self) {
        self.console.lock().separate_after_transition();
    }
}

impl Output for Logger {
    fn action(&self, verb: &str, subject: &str, planned: bool, message: &str) {
        let entry = super::buffered::entry::LogEntry::action(verb, subject, planned, message);
        if let Some(run) = &self.run_log {
            entry.persist(run);
        }
        entry.replay(self);
    }

    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
        if let Some(run_log) = &self.run_log {
            run_log.emit(kind.log_event(), &msg);
        }
        self.emit_console(kind, &msg);
        if kind == MsgKind::Startup {
            self.flush_startup_transition();
        }
    }

    fn finish_run(&self, code: i32) -> i32 {
        self.flush_startup_transition();
        let output_error = self.console.output_error();
        let mut final_code = output_error.as_ref().map_or(code, |error| {
            let message = format!("writing command output: {error}");
            if let Some(run) = &self.run_log {
                run.emit(LogEvent::Error, &message);
            }
            self.console.report_output_error(&message);
            1
        });
        if !self.finished.swap(true, Ordering::Relaxed) {
            self.console.record(
                &super::log_thread_name(),
                super::records::Record::RunFinish {
                    outcome: super::records::RunOutcome::from_exit_code(final_code),
                    elapsed_us: super::records::elapsed_us(self.start.elapsed()),
                    exit_code: final_code,
                },
            );
            if let Some(error) = self.console.output_error() {
                final_code = 1;
                if output_error.is_none() {
                    let message = format!("writing command output: {error}");
                    if let Some(run) = &self.run_log {
                        run.emit(LogEvent::Error, &message);
                    }
                    self.console.report_output_error(&message);
                }
            }
            if let Some(run) = &self.run_log {
                run.finish(
                    super::records::RunOutcome::from_exit_code(final_code),
                    final_code,
                );
            }
        }
        final_code
    }

    fn run_id(&self) -> Option<String> {
        self.console
            .lock()
            .run_id
            .clone()
            .or_else(|| self.run_log.as_ref().map(|run| run.id()))
    }

    fn run_log(&self) -> Option<&RunLog> {
        self.run_log.as_deref()
    }

    fn status_line(&self, msg: &str) {
        if !self.progress_terminal {
            return;
        }
        let _guard = self.lock_flush();
        self.replace_status_line(msg);
    }

    fn clear_status_line(&self) {
        self.clear_status();
    }
}

impl TaskRecorder for Logger {
    fn record_task(&self, task: TaskEntry) {
        Self::record_task(self, task);
    }

    fn record_task_duration(&self, task_id: &str, duration: Duration) {
        Self::record_task_duration(self, task_id, duration);
    }
}

#[cfg(test)]
mod tests;
