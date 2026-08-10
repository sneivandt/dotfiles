//! Console output buffering for parallel task execution.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use super::logger::{Logger, stdout_supports_progress};
use super::runlog::RunLog;
use super::types::{ActionCounts, MsgKind, Output, TaskRecorder, TaskStatus, TaskVisibility};
use super::utils::sort_action_runs;

mod entry;

use entry::{LogEntry, should_record_task_details};

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
            if self.inner.is_verbose() || entry.is_visible_in_non_verbose(TaskStatus::Ok, None) {
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
    #[cfg(test)]
    pub fn flush_and_complete(&self, task_name: &str, status: TaskStatus) {
        self.flush_and_complete_inner(None, task_name, status);
    }

    /// Flush an engine task selected by scheduler identity.
    pub fn flush_and_complete_by_id(&self, task_id: &str, task_name: &str, status: TaskStatus) {
        self.flush_and_complete_inner(Some(task_id), task_name, status);
    }

    fn flush_and_complete_inner(&self, task_id: Option<&str>, task_name: &str, status: TaskStatus) {
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
            if let Some(task_id) = task_id {
                self.inner
                    .record_task_details_by_id(task_id, task_name, detail_lines);
            } else {
                self.inner.record_task_details(task_name, detail_lines);
            }
        }

        let show_progress = stdout_supports_progress();
        let _guard = self.inner.lock_flush();
        if show_progress {
            self.inner.clear_progress();
        }
        let visible = task_id.map_or_else(
            || self.inner.task_is_visible(task_name),
            |task_id| self.inner.task_is_visible_by_id(task_id),
        );
        let task_message = task_id.map_or_else(
            || self.inner.recorded_task_message(task_name),
            |task_id| self.inner.recorded_task_message_by_id(task_id),
        );
        let message = task_message.as_deref();
        if !visible {
            // Internal task: the entries live in the run log only.
        } else if self.inner.is_verbose() {
            // Verbose accounts for every task, including the ones with nothing
            // to do, and replays the per-resource decisions behind that outcome.
            if let Some(task_id) = task_id {
                self.inner.emit_recorded_task_status_by_id(task_id);
            } else {
                self.inner.emit_recorded_task_status(task_name);
            }
            let mut replayed = false;
            for entry in &entries {
                if entry.replay_verbose(message) {
                    replayed = true;
                }
            }
            self.inner.note_replayed_details(replayed);
        } else if !matches!(status, TaskStatus::Ok | TaskStatus::NotApplicable) {
            let has_visible_entries = entries
                .iter()
                .any(|entry| entry.is_visible_in_non_verbose(status, message));
            if has_visible_entries {
                self.inner.separate_from_startup();
                for entry in &entries {
                    if entry.is_visible_in_non_verbose(status, message) {
                        entry.replay();
                    }
                }
                self.inner.mark_task_console_output();
            }
        }
        self.inner.remove_active_task_locked(task_name);
        if let Some(task_id) = task_id {
            self.inner.mark_task_completed_by_id(task_id);
        } else {
            self.inner.mark_task_completed(task_name);
        }
        if visible && !self.inner.is_verbose() && status != TaskStatus::NotApplicable {
            if let Some(task_id) = task_id {
                self.inner.emit_recorded_task_result_by_id(task_id);
            } else {
                self.inner.emit_recorded_task_result(task_name);
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
        visibility: TaskVisibility,
    ) {
        self.inner
            .record_task_with_metadata(name, status, message, actions, visibility);
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
        self.inner
            .record_task_with_identity(task_id, name, status, message, actions, visibility);
    }

    fn record_task_duration(&self, name: &str, duration: std::time::Duration) {
        self.inner.record_task_duration(name, duration);
    }

    fn record_task_duration_by_id(
        &self,
        task_id: &str,
        _name: &str,
        duration: std::time::Duration,
    ) {
        self.inner.record_task_duration_by_id(task_id, duration);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
