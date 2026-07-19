//! Buffered logger for parallel task execution.
use std::sync::{Arc, Mutex};

use super::diagnostic::{DiagEvent, DiagnosticLog};
use super::logger::{Logger, stdout_supports_progress};
use super::types::{ActionCounts, Output, TaskRecorder, TaskStatus};

/// A single buffered log entry, replayed when flushed.
#[derive(Debug, Clone)]
enum LogEntry {
    /// A stage header entry.
    Stage(String),
    /// An informational entry.
    Info(String),
    /// A debug entry.
    Debug(String),
    /// A warning entry.
    Warn(String),
    /// An error entry.
    Error(String),
    /// A dry-run entry.
    DryRun(String),
    /// An always-visible entry.
    Always(String),
}

impl LogEntry {
    /// Replay this entry to the console and log file via tracing.
    ///
    /// Does **not** write to the diagnostic log because the entry was
    /// already recorded there in real-time when it was buffered.
    fn replay(&self) {
        match self {
            Self::Stage(msg) => tracing::info!(target: "dotfiles::stage", "{msg}"),
            Self::Info(msg) => tracing::info!("{msg}"),
            Self::Debug(msg) => tracing::debug!("{msg}"),
            Self::Warn(msg) => tracing::warn!("{msg}"),
            Self::Error(msg) => tracing::error!("{msg}"),
            Self::DryRun(msg) => tracing::info!(target: "dotfiles::dry_run", "{msg}"),
            Self::Always(msg) => tracing::info!(target: "dotfiles::always", "{msg}"),
        }
    }

    fn detail_line(&self, status: TaskStatus) -> Option<&str> {
        match self {
            Self::Info(msg) | Self::DryRun(msg) | Self::Always(msg) => Some(msg),
            Self::Warn(msg) | Self::Error(msg) if status == TaskStatus::Failed => Some(msg),
            Self::Stage(_) | Self::Debug(_) | Self::Warn(_) | Self::Error(_) => None,
        }
    }

    fn replay_file_only(&self) {
        match self {
            Self::Stage(msg) => tracing::info!(target: "dotfiles::file_only_stage", "{msg}"),
            Self::Info(msg) => {
                tracing::info!(target: "dotfiles::file_only", "{msg}");
            }
            Self::Debug(msg) => tracing::info!(target: "dotfiles::file_only_debug", "{msg}"),
            Self::Warn(msg) => tracing::info!(target: "dotfiles::file_only_warn", "{msg}"),
            Self::Error(msg) => tracing::info!(target: "dotfiles::file_only_error", "{msg}"),
            Self::DryRun(msg) | Self::Always(msg) => {
                tracing::info!(target: "dotfiles::file_only", "{msg}");
            }
        }
    }

    fn replay_non_verbose(&self, status: TaskStatus) {
        if status == TaskStatus::Failed {
            self.replay_file_only();
            return;
        }

        match self {
            Self::Stage(_) | Self::Info(_) | Self::Debug(_) | Self::DryRun(_) | Self::Always(_) => {
                self.replay_file_only();
            }
            Self::Warn(_) | Self::Error(_) => self.replay(),
        }
    }

    fn is_visible_in_non_verbose(&self, status: TaskStatus) -> bool {
        status != TaskStatus::Failed && matches!(self, Self::Warn(_) | Self::Error(_))
    }
}

/// Implement the display methods of [`Output`] by buffering each message into
/// `self.entries` as the corresponding [`LogEntry`] variant.
///
/// Each method also forwards the message to the diagnostic log in real-time
/// (bypassing the buffer) so that the true chronological order of events
/// during parallel execution is preserved.
///
/// The `record_task` method is **not** included because it forwards to
/// `self.inner` instead of buffering.
macro_rules! buffer_log_methods {
    ($($method:ident => $variant:ident => $diag:ident),+ $(,)?) => {
        $(
            fn $method(&self, msg: &str) {
                if let Some(d) = &self.inner.diagnostic {
                    d.emit(DiagEvent::$diag, msg);
                }
                if let Ok(mut guard) = self.entries.lock() {
                    guard.push(LogEntry::$variant(msg.to_string()));
                }
            }
        )+
    };
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
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if self.inner.is_verbose() {
            for entry in &entries {
                entry.replay();
            }
        } else {
            for entry in &entries {
                entry.replay_non_verbose(TaskStatus::Ok);
            }
        }
    }

    /// Flush all buffered entries and remove the task from the active set.
    ///
    /// Acquires the flush lock on the backing [`Logger`] to prevent
    /// interleaved console output when multiple tasks complete concurrently.
    /// After replaying the buffered entries, appends the completed task result
    /// and updates the active-task display.
    ///
    /// In non-verbose mode verbose task output is written to the log file only,
    /// then a compact task result is written to the console immediately.
    #[allow(clippy::print_stderr, reason = "intentional user-facing output")]
    pub fn flush_and_complete(&self, task_name: &str, status: TaskStatus) {
        {
            let show_progress = stdout_supports_progress();
            let _guard = self.inner.flush_lock.lock().unwrap_or_else(|e| {
                eprintln!("warning: flush lock was poisoned, recovering");
                e.into_inner()
            });
            if show_progress {
                self.inner.clear_progress();
            }
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
            let span = tracing::info_span!("task", name = task_name);
            let _enter = span.enter();
            if self.inner.is_verbose() {
                for entry in &entries {
                    entry.replay();
                }
                if !entries.is_empty() {
                    self.inner.mark_task_console_output();
                }
            } else {
                let has_visible_entries = entries
                    .iter()
                    .any(|entry| entry.is_visible_in_non_verbose(status));
                if has_visible_entries {
                    self.inner.separate_from_startup();
                }
                for entry in &entries {
                    entry.replay_non_verbose(status);
                }
                if has_visible_entries {
                    self.inner.mark_task_console_output();
                }
            }
            self.inner.remove_active_task_locked(task_name);
            if !self.inner.is_verbose() {
                self.inner.emit_recorded_task_result(task_name);
            }
            self.inner.redraw_active_status_locked(show_progress);
        }
    }
}

const fn should_record_task_details(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed | TaskStatus::Skipped | TaskStatus::DryRun | TaskStatus::Failed
    )
}

impl Output for BufferedLog {
    buffer_log_methods! {
        stage   => Stage   => Stage,
        info    => Info    => Info,
        debug   => Debug   => Debug,
        warn    => Warn    => Warn,
        error   => Error   => Error,
        dry_run => DryRun  => DryRun,
        always  => Always  => Info,
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.inner.diagnostic.as_ref()
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
    fn buffered_log_flush_replays_to_file() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let marker = format!("buf-marker-{}", std::process::id());
        buf.info(&marker);
        let path = log.log_path().expect("log path");
        let before = fs::read_to_string(path).unwrap();
        assert!(
            !before.contains(&marker),
            "buffered output should not be written before flush"
        );
        buf.flush();
        let after = fs::read_to_string(path).unwrap();
        assert!(
            after.contains(&marker),
            "buffered output should appear after flush"
        );
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
    fn buffered_log_writes_to_diagnostic_immediately() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let marker = format!("buf-diag-{}", std::process::id());
        buf.info(&marker);
        let diag = log.diagnostic.as_ref().expect("diagnostic log");
        let contents = fs::read_to_string(diag.path()).unwrap();
        assert!(
            contents.contains(&marker),
            "BufferedLog should write to diagnostic immediately, not after flush"
        );
    }

    #[test]
    fn log_entry_replay_all_variants() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let pid = std::process::id();
        buf.stage(&format!("replay-stage-{pid}"));
        buf.info(&format!("replay-info-{pid}"));
        buf.debug(&format!("replay-debug-{pid}"));
        buf.warn(&format!("replay-warn-{pid}"));
        buf.error(&format!("replay-error-{pid}"));
        buf.dry_run(&format!("replay-dryrun-{pid}"));
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
        buf.info(&format!("all-info-{pid}"));
        buf.warn(&format!("all-warn-{pid}"));
        buf.error(&format!("all-error-{pid}"));
        buf.dry_run(&format!("all-dryrun-{pid}"));
        buf.debug(&format!("all-debug-{pid}"));
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
    fn non_verbose_replay_only_demotes_verbose_detail_entries() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let targets = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(TargetCaptureLayer {
            targets: Arc::clone(&targets),
        });
        let dispatch = tracing::Dispatch::new(subscriber);
        let _guard = crate::infra::logging::test_dispatch_guard(&dispatch);

        for entry in [
            LogEntry::Stage("stage".to_string()),
            LogEntry::Info("info".to_string()),
            LogEntry::Debug("debug".to_string()),
            LogEntry::Warn("warn".to_string()),
            LogEntry::Error("error".to_string()),
            LogEntry::DryRun("dry-run".to_string()),
            LogEntry::Always("always".to_string()),
        ] {
            entry.replay_non_verbose(TaskStatus::Ok);
        }

        let targets = targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(targets[0], "dotfiles::file_only_stage");
        assert_eq!(targets[1], "dotfiles::file_only");
        assert_eq!(targets[2], "dotfiles::file_only_debug");
        assert!(
            !targets.contains(&"dotfiles::dry_run".to_string()),
            "dry-run task details should be deferred to the summary in non-verbose replay: {targets:?}"
        );
        assert!(
            !targets.contains(&"dotfiles::always".to_string()),
            "always task details should be deferred to the summary in non-verbose replay: {targets:?}"
        );
        assert!(
            !targets.iter().any(|target| matches!(
                target.as_str(),
                "dotfiles::file_only_warn" | "dotfiles::file_only_error"
            )),
            "warnings and errors must not be demoted to file-only replay: {targets:?}"
        );
    }

    #[test]
    fn non_verbose_failed_replay_keeps_errors_file_only() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let targets = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(TargetCaptureLayer {
            targets: Arc::clone(&targets),
        });
        let dispatch = tracing::Dispatch::new(subscriber);
        let _guard = crate::infra::logging::test_dispatch_guard(&dispatch);

        LogEntry::Warn("warn".to_string()).replay_non_verbose(TaskStatus::Failed);
        LogEntry::Error("error".to_string()).replay_non_verbose(TaskStatus::Failed);

        let targets = targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            targets,
            vec!["dotfiles::file_only_warn", "dotfiles::file_only_error"],
            "failed-task errors should be persisted without separate console lines"
        );
    }

    #[test]
    fn failed_task_errors_become_task_details() {
        let warning = LogEntry::Warn("failed: package install".to_string());
        let error = LogEntry::Error("packages: command failed".to_string());

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
    fn buffered_log_diagnostic_returns_inner_diag() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));
        let buf_diag = buf.diagnostic();
        let log_diag = log.diagnostic.as_ref();
        assert_eq!(
            buf_diag.is_some(),
            log_diag.is_some(),
            "BufferedLog::diagnostic() should match the logger's diagnostic"
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
    /// `stats.finish(ctx)` — must have their `==>` stage header replayed
    /// by `flush_and_complete()`.
    ///
    /// Before this was caught, tasks producing `"0 changed, X already ok"`
    /// output were observed without their stage headers in the console.
    #[test]
    fn flush_and_complete_replays_stage_before_info() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        let buf = BufferedLog::new(Arc::clone(&log));

        // Simulate the order execute() + stats.finish(ctx) produce entries:
        // execute() calls ctx.log().stage() first, then run() calls ctx.log().info()
        // via stats.finish() before returning Ok.
        buf.stage("install-task");
        buf.info("0 changed, 37 already ok");

        buf.flush_and_complete("install-task", TaskStatus::Ok);

        let path = log.log_path().expect("log path");
        let contents = fs::read_to_string(path).unwrap();

        let stage_pos = contents
            .find("==> install-task")
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
            contents.contains("==> parallel-task"),
            "stage header must appear after flush_and_complete even when progress row was active\nlog:\n{contents}"
        );
        assert!(
            contents.contains("0 changed, 1 already ok"),
            "stats info must appear\nlog:\n{contents}"
        );
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
            !contents.contains("[dry-run]"),
            "dry-run detail replay should not add a redundant dry-run tag\nlog:\n{contents}"
        );
        assert!(
            contents.contains("] [Configure Copilot] [info] would configure beep = true"),
            "dry-run detail replay should use the info text level in the persistent log\nlog:\n{contents}"
        );
    }
}
