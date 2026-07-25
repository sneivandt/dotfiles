//! Console output buffering for parallel task execution.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use super::logger::{Logger, stdout_supports_progress};
use super::runlog::RunLog;
use super::types::{ActionCounts, MsgKind, Output, TaskRecorder, TaskStatus, emit_console_event};

/// A single buffered console entry, replayed when the task completes.
///
/// Only entries that can reach the console are buffered.  Everything is
/// already recorded in the run log at the moment it is produced, so replay is
/// purely a console-rendering concern.
#[derive(Debug, Clone)]
struct LogEntry {
    /// What kind of message this is.
    kind: MsgKind,
    /// The message text, with the caller's allocation taken over.
    msg: String,
}

impl LogEntry {
    /// Replay this entry to the console via tracing.
    fn replay(&self) {
        emit_console_event!(self.kind, &self.msg);
    }

    /// Replay this entry as verbose task detail.
    ///
    /// Task-name headers are suppressed because the task status line already
    /// names the task.
    fn replay_verbose(&self) {
        if self.kind != MsgKind::TaskStage {
            self.replay();
        }
    }

    /// The summary detail line contributed by this entry, if any.
    fn detail_line(&self, status: TaskStatus) -> Option<&str> {
        match self.kind {
            MsgKind::Info | MsgKind::DryRun | MsgKind::Always => Some(&self.msg),
            MsgKind::Warn | MsgKind::Error if status == TaskStatus::Failed => Some(&self.msg),
            MsgKind::Stage
            | MsgKind::TaskStage
            | MsgKind::Debug
            | MsgKind::Warn
            | MsgKind::Error => None,
        }
    }

    /// Whether this entry appears on the console in non-verbose mode.
    ///
    /// A failed task reports through its summary entry instead, so its
    /// buffered output stays in the run log only.
    fn is_visible_in_non_verbose(&self, status: TaskStatus) -> bool {
        status != TaskStatus::Failed && matches!(self.kind, MsgKind::Warn | MsgKind::Error)
    }
}

/// Buffered logger for parallel task execution.
///
/// Captures display output (stage, info, debug, etc.) in memory so that
/// parallel tasks do not interleave their console output.  The captured
/// entries are replayed in order when `flush_and_complete` is called.
///
/// [`record_task`](crate::infra::logging::TaskRecorder::record_task) is forwarded directly to the underlying
/// [`Logger`] because the summary collection is already thread-safe.
#[derive(Debug)]
pub struct BufferedLog {
    inner: Arc<Logger>,
    entries: Mutex<Vec<LogEntry>>,
}

impl BufferedLog {
    /// Create a new buffered logger backed by the given [`Logger`].
    #[must_use]
    pub const fn new(inner: Arc<Logger>) -> Self {
        Self {
            inner,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Replay all buffered entries to the backing [`Logger`].
    #[cfg(test)]
    pub fn flush(&self) {
        let entries = std::mem::take(
            &mut *self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for entry in &entries {
            if self.inner.is_verbose() || entry.is_visible_in_non_verbose(TaskStatus::Ok) {
                entry.replay();
            }
        }
    }

    /// Flush buffered console output and remove the task from the active set.
    ///
    /// Acquires the flush lock on the backing [`Logger`] to prevent
    /// interleaved console output when multiple tasks complete concurrently.
    /// After replaying the buffered entries, appends the completed task result
    /// and updates the active-task display.
    ///
    /// Entries are already present in the run log, so anything not replayed
    /// here is simply not shown on the console.
    #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
    pub fn flush_and_complete(&self, task_name: &str, status: TaskStatus) {
        let entries = {
            let mut guard = self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        if should_record_task_details(status) {
            let detail_lines: Vec<String> = entries
                .iter()
                .filter_map(|entry| entry.detail_line(status))
                .map(ToString::to_string)
                .collect();
            self.inner.record_task_details(task_name, detail_lines);
        }

        let show_progress = stdout_supports_progress();
        let _guard = self.inner.flush_lock.lock().unwrap_or_else(|e| {
            eprintln!("warning: flush lock was poisoned, recovering");
            e.into_inner()
        });
        if show_progress {
            self.inner.clear_progress();
        }
        let visible = self.inner.task_is_visible(task_name);
        if matches!(status, TaskStatus::Ok | TaskStatus::NotApplicable) || !visible {
            // Nothing to show: the entries live in the run log only.
        } else if self.inner.is_verbose() {
            self.inner.emit_recorded_task_status(task_name);
            for entry in &entries {
                entry.replay_verbose();
            }
        } else {
            let has_visible_entries = entries
                .iter()
                .any(|entry| entry.is_visible_in_non_verbose(status));
            if has_visible_entries {
                self.inner.separate_from_startup();
                for entry in &entries {
                    if entry.is_visible_in_non_verbose(status) {
                        entry.replay();
                    }
                }
                self.inner.mark_task_console_output();
            }
        }
        self.inner.remove_active_task_locked(task_name);
        if visible && !self.inner.is_verbose() && status != TaskStatus::NotApplicable {
            self.inner.emit_recorded_task_result(task_name);
        }
        self.inner.redraw_active_status_locked(show_progress);
    }
}

const fn should_record_task_details(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed | TaskStatus::Skipped | TaskStatus::DryRun | TaskStatus::Failed
    )
}

impl Output for BufferedLog {
    /// Record the message in the run log immediately and buffer it for later
    /// console replay.
    ///
    /// Writing to the run log first (rather than at flush time) is what
    /// preserves the true chronological order of events during parallel
    /// execution.  Debug messages are never console-visible and are never used
    /// for task detail lines, so they are not buffered at all.
    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
        if let Some(run_log) = &self.inner.run_log {
            run_log.emit(kind.log_event(), &msg);
        }
        if kind == MsgKind::Debug {
            return;
        }
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(LogEntry {
                kind,
                msg: msg.into_owned(),
            });
        }
    }

    fn run_log(&self) -> Option<&RunLog> {
        self.inner.run_log.as_deref()
    }
}

impl TaskRecorder for BufferedLog {
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.inner.record_task(name, status, message);
    }

    fn record_task_with_actions(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
    ) {
        self.inner
            .record_task_with_actions(name, status, message, actions);
    }

    fn record_task_with_metadata(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        visible: bool,
    ) {
        self.inner
            .record_task_with_metadata(name, status, message, actions, visible);
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
    use super::super::types::OutputExt as _;
    use super::*;
    use crate::infra::logging::isolated_logger;

    /// Build a buffered entry of the given kind for replay assertions.
    fn entry(kind: MsgKind, msg: &str) -> LogEntry {
        LogEntry {
            kind,
            msg: msg.to_string(),
        }
    }
    use std::fs;
    use std::sync::Arc;

    #[test]
    fn buffered_log_record_task_forwards_to_logger() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.record_task("task-a", TaskStatus::Ok, None);
        assert_eq!(log.task_entries().len(), 1);
        assert_eq!(log.task_entries()[0].name, "task-a");
    }

    #[test]
    fn buffered_log_record_task_with_actions_forwards_counts() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let actions = ActionCounts {
            applied: 2,
            ..ActionCounts::default()
        };

        buf.record_task_with_actions("task-a", TaskStatus::Changed, None, actions);

        assert_eq!(log.task_entries()[0].actions, actions);
    }

    #[test]
    fn buffered_log_preserves_entry_order() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.stage("stage-1");
        buf.info("info-1");
        buf.debug("debug-1");
        buf.warn("warn-1");
        buf.flush();
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        let stage_pos = contents.find("stage-1").expect("stage-1 in log");
        let info_pos = contents.find("info-1").expect("info-1 in log");
        let debug_pos = contents.find("debug-1").expect("debug-1 in log");
        let warn_pos = contents.find("warn-1").expect("warn-1 in log");
        assert!(stage_pos < info_pos, "stage before info");
        assert!(info_pos < debug_pos, "info before debug");
        assert!(debug_pos < warn_pos, "debug before warn");
    }

    #[test]
    fn flush_and_complete_clears_progress_rows() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("update");
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.flush_and_complete("update", TaskStatus::Ok);
        assert_eq!(
            log.progress_rows_count(),
            0,
            "progress_rows should be zero after all tasks complete"
        );
    }

    #[test]
    fn buffered_log_writes_to_run_log_immediately() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let marker = format!("buf-runlog-{}", std::process::id());
        buf.info(&marker);
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(&marker),
            "BufferedLog should write to the run log immediately, not after flush"
        );
    }

    #[test]
    fn log_entry_replay_all_variants() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let pid = std::process::id();
        buf.stage(format!("replay-stage-{pid}"));
        buf.info(format!("replay-info-{pid}"));
        buf.debug(format!("replay-debug-{pid}"));
        buf.warn(format!("replay-warn-{pid}"));
        buf.error(format!("replay-error-{pid}"));
        buf.dry_run(format!("replay-dryrun-{pid}"));
        buf.flush();
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains(&format!("replay-stage-{pid}")));
        assert!(contents.contains(&format!("replay-info-{pid}")));
        assert!(contents.contains(&format!("replay-debug-{pid}")));
        assert!(contents.contains(&format!("replay-warn-{pid}")));
        assert!(contents.contains(&format!("replay-error-{pid}")));
        assert!(contents.contains(&format!("replay-dryrun-{pid}")));
    }

    #[test]
    fn buffered_log_all_variants_buffered() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let pid = std::process::id();
        buf.info(format!("all-info-{pid}"));
        buf.warn(format!("all-warn-{pid}"));
        buf.error(format!("all-error-{pid}"));
        buf.dry_run(format!("all-dryrun-{pid}"));
        buf.debug(format!("all-debug-{pid}"));
        buf.flush();
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains(&format!("all-info-{pid}")));
        assert!(contents.contains(&format!("all-warn-{pid}")));
        assert!(contents.contains(&format!("all-error-{pid}")));
        assert!(contents.contains(&format!("all-dryrun-{pid}")));
        assert!(contents.contains(&format!("all-debug-{pid}")));
    }

    #[derive(Clone, Debug)]
    struct TargetCaptureLayer {
        targets: Arc<Mutex<Vec<String>>>,
    }

    impl<S> tracing_subscriber::Layer<S> for TargetCaptureLayer
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.metadata().target().to_string());
        }
    }

    #[test]
    fn non_verbose_replay_only_shows_warnings_and_errors() {
        for entry in [
            entry(MsgKind::Stage, "stage"),
            entry(MsgKind::TaskStage, "task"),
            entry(MsgKind::Info, "info"),
            entry(MsgKind::DryRun, "dry-run"),
            entry(MsgKind::Always, "always"),
        ] {
            assert!(
                !entry.is_visible_in_non_verbose(TaskStatus::Ok),
                "{entry:?} should be deferred to the summary in non-verbose replay"
            );
        }

        for entry in [entry(MsgKind::Warn, "warn"), entry(MsgKind::Error, "error")] {
            assert!(
                entry.is_visible_in_non_verbose(TaskStatus::Ok),
                "{entry:?} must reach the console in non-verbose replay"
            );
        }
    }

    #[test]
    fn non_verbose_failed_replay_keeps_errors_off_the_console() {
        for entry in [entry(MsgKind::Warn, "warn"), entry(MsgKind::Error, "error")] {
            assert!(
                !entry.is_visible_in_non_verbose(TaskStatus::Failed),
                "failed-task output surfaces through the summary, not a separate console line"
            );
        }
    }

    #[test]
    fn verbose_replay_targets_console_ui() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let targets = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(TargetCaptureLayer {
            targets: Arc::clone(&targets),
        });
        let dispatch = tracing::Dispatch::new(subscriber);
        let _guard = crate::infra::logging::test_dispatch_guard(&dispatch);

        for entry in [
            entry(MsgKind::Stage, "stage"),
            entry(MsgKind::TaskStage, "task"),
            entry(MsgKind::Info, "info"),
            entry(MsgKind::Warn, "warn"),
        ] {
            entry.replay_verbose();
        }

        let targets = targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            targets,
            vec![
                "dotfiles::ui::stage",
                "dotfiles::ui::info",
                "dotfiles::ui::warn"
            ],
            "verbose replay renders console targets and suppresses the task-name header"
        );
    }

    #[test]
    fn failed_task_errors_become_task_details() {
        let warning = entry(MsgKind::Warn, "failed: package install");
        let error = entry(MsgKind::Error, "packages: command failed");

        assert_eq!(
            warning.detail_line(TaskStatus::Failed),
            Some("failed: package install")
        );
        assert_eq!(
            error.detail_line(TaskStatus::Failed),
            Some("packages: command failed")
        );
        assert_eq!(warning.detail_line(TaskStatus::Ok), None);
        assert_eq!(error.detail_line(TaskStatus::Ok), None);
    }

    #[test]
    fn buffered_log_run_log_returns_inner_run_log() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        assert_eq!(
            buf.run_log().is_some(),
            log.run_log().is_some(),
            "BufferedLog::run_log() should match the logger's run log"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening, reason = "intentional lock scope")]
    fn buffered_flush_and_complete_with_remaining_task() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.flush_and_complete("task-a", TaskStatus::Ok);
        let active = log.active_tasks.lock().unwrap();
        assert!(
            active.contains(&"task-b".to_string()),
            "task-b should still be in active tasks"
        );
    }

    /// Regression test: tasks that produce stats output by calling
    /// `ctx.log().info()` inside `run()` — as `process_resources` does via
    /// central stats reporting — must have their stage header recorded
    /// by `flush_and_complete()`.
    ///
    /// Before this was caught, tasks producing `"0 changed, X already ok"`
    /// output were observed without their stage headers in the persistent log.
    #[test]
    fn flush_and_complete_replays_stage_before_info() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));

        // Simulate the order execute() and central stats reporting produce entries:
        // execute() calls ctx.log().stage() first, then run() calls ctx.log().info()
        // via stats.finish() before returning Ok.
        buf.stage("install-task");
        buf.info("0 changed, 37 already ok");

        buf.flush_and_complete("install-task", TaskStatus::Ok);

        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();

        let stage_pos = contents
            .find("[stage] install-task")
            .expect("stage header must appear in log after flush_and_complete");
        let info_pos = contents
            .find("0 changed, 37 already ok")
            .expect("stats info must appear in log after flush_and_complete");

        assert!(
            stage_pos < info_pos,
            "stage header must come before stats info\nlog:\n{contents}"
        );
    }

    /// Regression test: the stage header must appear even when `notify_task_start`
    /// has been called first (i.e., a progress row is active), as happens in the
    /// parallel scheduler where `notify_task_start` precedes `execute()`.
    #[test]
    fn flush_and_complete_replays_stage_after_progress_clear() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);

        // Simulate parallel scheduler: notify_task_start before execute().
        log.notify_task_start("parallel-task");

        let buf = BufferedLog::new(Arc::clone(&log));
        buf.stage("parallel-task");
        buf.info("0 changed, 1 already ok");

        buf.flush_and_complete("parallel-task", TaskStatus::Ok);

        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();

        assert!(
            contents.contains("[stage] parallel-task"),
            "stage header must appear after flush_and_complete even when progress row was active\nlog:\n{contents}"
        );
        assert!(
            contents.contains("0 changed, 1 already ok"),
            "stats info must appear\nlog:\n{contents}"
        );
    }

    #[test]
    fn verbose_flush_keeps_not_applicable_task_output_off_console() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.task_stage("windows-only-task");
        buf.debug("not applicable: requires Windows");

        buf.flush_and_complete("windows-only-task", TaskStatus::NotApplicable);

        assert!(!log.task_console_output_emitted());
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("windows-only-task"));
        assert!(contents.contains("not applicable: requires Windows"));
    }

    #[test]
    fn verbose_flush_keeps_unchanged_task_output_off_console() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.task_stage("current-task");
        buf.info("0 changed, 1 already ok");

        buf.flush_and_complete("current-task", TaskStatus::Ok);

        assert!(!log.task_console_output_emitted());
        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("current-task"));
        assert!(contents.contains("0 changed, 1 already ok"));
    }

    #[test]
    fn non_verbose_dry_run_flush_keeps_detail_in_persistent_log() {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(false);
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));

        buf.dry_run("would configure beep = true");
        buf.flush_and_complete("Configure Copilot", TaskStatus::DryRun);

        let details = log
            .task_details
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].name, "Configure Copilot");
        assert_eq!(details[0].lines, ["would configure beep = true"]);

        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains("would configure beep = true"),
            "dry-run details should still be written to the persistent log"
        );
        assert!(
            contents.contains("[dry_run] would configure beep = true"),
            "dry-run details are recorded under the dry_run event kind\nlog:\n{contents}"
        );
    }
}
