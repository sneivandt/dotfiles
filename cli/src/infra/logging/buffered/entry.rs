//! Buffered console entries produced during parallel task execution.
//!
//! Splitting the entry model out of [`BufferedLog`](super::BufferedLog) keeps
//! replay/visibility policy (what a single entry means for the console and for
//! summary details) separate from the buffering and flush orchestration.

use crate::infra::logging::types::{MsgKind, TaskStatus, emit_console_event};
use crate::infra::logging::utils::{duplicates_task_message, is_stats_summary};

/// A single buffered console entry, replayed when the task completes.
///
/// Only entries that can reach the console are buffered.  Everything is
/// already recorded in the run log at the moment it is produced, so replay is
/// purely a console-rendering concern.
#[derive(Debug, Clone)]
pub(super) struct LogEntry {
    /// What kind of message this is.
    pub(super) kind: MsgKind,
    /// The message text, with the caller's allocation taken over.
    pub(super) msg: String,
}

impl LogEntry {
    /// Replay this entry to the console via tracing.
    pub(super) fn replay(&self) {
        emit_console_event!(self.kind, &self.msg);
    }

    /// Replay this entry as verbose task detail, reporting whether it printed.
    ///
    /// Task-name headers are suppressed because the task status line already
    /// names the task, as are lines that only restate the task's own outcome:
    /// its reason (already on the status row) and its aggregate counters
    /// (already implied by the per-item lines around them).
    pub(super) fn replay_verbose(&self, task_message: Option<&str>) -> bool {
        if matches!(self.kind, MsgKind::TaskStage | MsgKind::Stage)
            || duplicates_task_message(&self.msg, task_message)
            || is_stats_summary(&self.msg)
        {
            return false;
        }
        self.replay();
        true
    }

    /// The summary detail line contributed by this entry, if any.
    pub(super) fn detail_line(&self, status: TaskStatus) -> Option<&str> {
        match self.kind {
            MsgKind::Info | MsgKind::DryRun | MsgKind::Always => Some(&self.msg),
            MsgKind::Warn | MsgKind::Error if status == TaskStatus::Failed => Some(&self.msg),
            MsgKind::Stage
            | MsgKind::TaskStage
            | MsgKind::Debug
            | MsgKind::Startup
            | MsgKind::Warn
            | MsgKind::Error => None,
        }
    }

    /// Whether this entry appears on the console in non-verbose mode.
    ///
    /// A failed task reports through its summary entry instead, so its
    /// buffered output stays in the run log only.  Warnings that merely repeat
    /// the task's own reason are dropped as well: that reason is already on the
    /// task's status row, and printing it again on stderr detaches it from the
    /// task it belongs to.
    pub(super) fn is_visible_in_non_verbose(
        &self,
        status: TaskStatus,
        task_message: Option<&str>,
    ) -> bool {
        status != TaskStatus::Failed
            && matches!(self.kind, MsgKind::Warn | MsgKind::Error)
            && !duplicates_task_message(&self.msg, task_message)
    }
}

pub(super) const fn should_record_task_details(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed | TaskStatus::Skipped | TaskStatus::DryRun | TaskStatus::Failed
    )
}
