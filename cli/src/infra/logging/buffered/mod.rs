//! Console output buffering for parallel task execution.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use super::logger::{Logger, stdout_supports_progress};
use super::runlog::RunLog;
use super::types::{MsgKind, Output, TaskRecorder, TaskStatus};
use super::utils::sort_action_runs;

mod entry;

use entry::{LogEntry, should_record_task_details};

/// Buffered logger for parallel task execution.
///
/// Captures display output (stage, info, debug, etc.) in memory so that
/// parallel tasks do not interleave their console output.  The captured
/// entries are replayed in order when `flush_and_complete` is called.
///
/// Task results are forwarded directly to the underlying [`Logger`] because
/// summary collection is already thread-safe.
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
            if self.inner.is_verbose() || entry.is_visible_in_non_verbose(TaskStatus::Ok, None) {
                entry.replay(&self.inner);
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
    pub fn flush_and_complete(&self, task_id: &str, task_name: &str, status: TaskStatus) {
        let mut entries = {
            let mut guard = self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        // Parallel resource processing finishes in a nondeterministic order, so
        // sort the action lines before they reach either console path.
        sort_action_runs(&mut entries, |entry| entry.msg.as_str());
        if should_record_task_details(status) {
            let detail_lines: Vec<String> = entries
                .iter()
                .filter_map(|entry| entry.detail_line(status))
                .map(ToString::to_string)
                .collect();
            self.inner.record_task_details(task_id, detail_lines);
        }

        let show_progress = stdout_supports_progress();
        let _guard = self.inner.lock_flush();
        if show_progress {
            self.inner.clear_progress();
        }
        let visible = self.inner.task_is_visible(task_id);
        let task_message = self.inner.recorded_task_message(task_id);
        let message = task_message.as_deref();
        if !visible {
            // Internal task: the entries live in the run log only.
        } else if self.inner.is_verbose() {
            // Verbose accounts for every task, including the ones with nothing
            // to do, and replays the per-resource decisions behind that outcome.
            self.inner.emit_recorded_task_status(task_id);
            for entry in &entries {
                entry.replay_verbose(&self.inner, message);
            }
        }
        self.inner.remove_active_task_locked(task_name);
        self.inner.mark_task_completed(task_id);
        if visible && !self.inner.is_verbose() && status != TaskStatus::NotApplicable {
            self.inner.emit_recorded_task_result(task_id);
            // Keep warnings inside their task's block, after its status and
            // actions, so the next separator belongs to the next task.
            if status != TaskStatus::Ok {
                for entry in &entries {
                    if entry.is_visible_in_non_verbose(status, message) {
                        entry.replay(&self.inner);
                    }
                }
            }
        }
        self.inner.redraw_active_status_locked(show_progress);
    }
}

impl Output for BufferedLog {
    /// Record the message in the run log immediately and buffer it for later
    /// console replay.
    ///
    /// Writing to the run log first (rather than at flush time) is what
    /// preserves the true chronological order of events during parallel
    /// execution.  Debug messages are buffered too: they never reach the
    /// console in non-verbose mode and never become task detail lines, but
    /// verbose replays them as the per-resource reasoning behind an outcome.
    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
        if let Some(run_log) = &self.inner.run_log {
            run_log.emit(kind.log_event(), &msg);
        }
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(LogEntry {
                kind,
                msg: msg.into_owned(),
            });
    }

    fn run_log(&self) -> Option<&RunLog> {
        self.inner.run_log.as_deref()
    }
}

impl TaskRecorder for BufferedLog {
    fn record_task(&self, task: super::TaskEntry) {
        self.inner.record_task(task);
    }

    fn record_task_duration(&self, task_id: &str, duration: std::time::Duration) {
        self.inner.record_task_duration(task_id, duration);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
