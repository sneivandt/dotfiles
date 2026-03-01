//! Buffered logger for parallel task execution.
use std::sync::{Arc, Mutex};

use super::diagnostic::{DiagEvent, DiagnosticLog};
use super::logger::Logger;
use super::types::{Log, TaskStatus};

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
        }
    }
}

/// Implement the display methods of [`Log`] by buffering each message into
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
/// [`record_task`](Log::record_task) is forwarded directly to the underlying
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
        let entries = match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        for entry in &entries {
            entry.replay();
        }
    }

    /// Flush all buffered entries and remove the task from the active set.
    ///
    /// Acquires the flush lock on the backing [`Logger`] to prevent
    /// interleaved console output when multiple tasks complete concurrently.
    /// After replaying the buffered entries, updates the active task display.
    pub fn flush_and_complete(&self, task_name: &str) {
        let _guard = self
            .inner
            .flush_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.clear_progress();
        let entries = match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        for entry in &entries {
            entry.replay();
        }
        let remaining = self.inner.active_tasks.lock().ok().and_then(|mut active| {
            active.retain(|n| n != task_name);
            (!active.is_empty()).then(|| active.join(", "))
        });
        if let Some(names) = remaining {
            self.inner.draw_progress(&names);
        }
    }
}

impl Log for BufferedLog {
    buffer_log_methods! {
        stage   => Stage   => Stage,
        info    => Info    => Info,
        debug   => Debug   => Debug,
        warn    => Warn    => Warn,
        error   => Error   => Error,
        dry_run => DryRun  => DryRun,
    }

    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>) {
        self.inner.record_task(name, status, message);
    }

    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        self.inner.diagnostic.as_ref()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::logging::isolated_logger;
    use std::fs;

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
        buf.flush_and_complete("update");
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
    #[allow(clippy::significant_drop_tightening)]
    fn buffered_flush_and_complete_with_remaining_task() {
        let (log, _tmp, _guard) = isolated_logger();
        let log = Arc::new(log);
        log.notify_task_start("task-a");
        log.notify_task_start("task-b");
        let buf = BufferedLog::new(Arc::clone(&log));
        buf.flush_and_complete("task-a");
        let active = log.active_tasks.lock().unwrap();
        assert!(
            active.contains(&"task-b".to_string()),
            "task-b should still be in active tasks"
        );
    }
}
