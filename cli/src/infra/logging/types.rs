//! Core logging types: task entries, status, and the [`Log`] trait.
use super::diagnostic::{DiagEvent, DiagnosticLog};
use super::style::TextStyle;

/// Structured action totals contributed by a task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionCounts {
    /// Actions applied to the system.
    pub applied: u32,
    /// Actions planned during a dry run.
    pub planned: u32,
    /// Actions deliberately skipped.
    pub skipped: u32,
    /// Actions that failed.
    pub failed: u32,
}

impl ActionCounts {
    /// Merge another set of action totals, saturating each counter.
    pub const fn merge(&mut self, other: Self) {
        self.applied = self.applied.saturating_add(other.applied);
        self.planned = self.planned.saturating_add(other.planned);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.failed = self.failed.saturating_add(other.failed);
    }

    /// Return whether all action counters are zero.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.applied == 0 && self.planned == 0 && self.skipped == 0 && self.failed == 0
    }
}

/// Task execution result for summary reporting.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// Human-readable task name.
    pub name: String,
    /// Final status of the task.
    pub status: TaskStatus,
    /// Optional detail message (e.g., skip reason or error description).
    pub message: Option<String>,
    /// Structured action totals produced by the task.
    pub actions: ActionCounts,
}

/// Status of a completed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task completed successfully and changed system state.
    Changed,
    /// Task completed successfully without a recorded state change.
    Ok,
    /// Task was skipped because it does not apply to the current platform or profile.
    NotApplicable,
    /// Task was explicitly skipped (e.g., tool not found, config empty).
    Skipped,
    /// Task would change state in dry-run mode; no changes were applied.
    DryRun,
    /// Task encountered an error and could not complete.
    Failed,
}

impl TaskStatus {
    /// Text style used for compact status rendering.
    #[must_use]
    pub(in crate::infra::logging) const fn text_style(self) -> Option<TextStyle> {
        match self {
            Self::Changed => Some(TextStyle::Green),
            Self::Ok => Some(TextStyle::Dim),
            Self::Skipped => Some(TextStyle::Yellow),
            Self::DryRun => Some(TextStyle::Magenta),
            Self::Failed => Some(TextStyle::Red),
            Self::NotApplicable => None,
        }
    }
}

/// User-facing output methods.
///
/// This trait covers display-oriented logging: stage headers, informational
/// messages, debug output, warnings, errors, and dry-run annotations. It
/// intentionally excludes structured task recording, which belongs to
/// [`TaskRecorder`].
///
/// Both [`Logger`](super::logger::Logger) and
/// [`BufferedLog`](super::buffered::BufferedLog) implement this trait.
pub trait Output: Send + Sync {
    /// Log a stage header (major section).
    fn stage(&self, msg: &str);
    /// Log a task name without major-section emphasis.
    fn task_stage(&self, msg: &str) {
        self.stage(msg);
    }
    /// Log an informational message.
    fn info(&self, msg: &str);
    /// Log a debug message (may be suppressed on console).
    fn debug(&self, msg: &str);
    /// Log a warning message.
    fn warn(&self, msg: &str);
    /// Log an error message.
    fn error(&self, msg: &str);
    /// Log a dry-run action message.
    fn dry_run(&self, msg: &str);
    /// Log a message that always appears on the console regardless of verbose
    /// setting.  Used for structural output (version, profile, summary).
    fn always(&self, msg: &str);
    /// Return whether debug logging is currently active on this thread.
    ///
    /// This intentionally avoids `tracing::enabled!`, which can leave stale
    /// per-layer filter state behind on replay paths.  The default
    /// implementation only checks whether a tracing dispatcher has been set,
    /// which is enough for this codebase because command execution installs a
    /// DEBUG-capable file layer whenever logging is active.
    fn debug_enabled(&self) -> bool {
        tracing::dispatcher::has_been_set()
    }
    /// Access the high-precision diagnostic log, if available.
    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        None
    }
    /// Emit a diagnostic event when the diagnostic log is enabled.
    ///
    /// This is a convenience wrapper around
    /// [`DiagnosticLog::emit`](super::DiagnosticLog::emit) that no-ops when
    /// no diagnostic log is configured, so call sites do not need to
    /// `if let Some(diag) = ...` themselves.
    fn diag(&self, event: DiagEvent, message: &str) {
        if let Some(diag) = self.diagnostic() {
            diag.emit(event, message);
        }
    }
    /// Emit a task-scoped diagnostic event when the diagnostic log is enabled.
    ///
    /// Convenience wrapper around
    /// [`DiagnosticLog::emit_task`](super::DiagnosticLog::emit_task).
    fn diag_task(&self, event: DiagEvent, task: &str, message: &str) {
        if let Some(diag) = self.diagnostic() {
            diag.emit_task(event, task, message);
        }
    }
}

/// Structured task result recording for summary reports.
///
/// Separated from [`Output`] so that resource-processing code can depend
/// only on display methods while the scheduler records task outcomes
/// independently.
pub trait TaskRecorder: Send + Sync {
    /// Record a task result for the summary.
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>);

    /// Record a task result and its structured action totals.
    ///
    /// The default preserves compatibility with recorders that only collect
    /// task-level outcomes.
    fn record_task_with_actions(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        _actions: ActionCounts,
    ) {
        self.record_task(name, status, message);
    }
}

/// Combined logging interface: user-facing output plus task recording.
///
/// This is the primary trait stored in the execution `Context`.
/// It composes [`Output`] (display methods) and [`TaskRecorder`] (structured
/// task results), allowing callers that need the full interface to accept a
/// single trait object.
///
/// A blanket implementation is provided for any type that implements both
/// sub-traits, so concrete types only need to implement [`Output`] and
/// [`TaskRecorder`].
pub trait Log: Output + TaskRecorder {}

impl<T: Output + TaskRecorder> Log for T {}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn task_status_equality() {
        assert_eq!(TaskStatus::Ok, TaskStatus::Ok);
        assert_eq!(TaskStatus::Changed, TaskStatus::Changed);
        assert_eq!(TaskStatus::Failed, TaskStatus::Failed);
        assert_ne!(TaskStatus::Ok, TaskStatus::Failed);
        assert_ne!(TaskStatus::Changed, TaskStatus::Ok);
        assert_ne!(TaskStatus::Skipped, TaskStatus::DryRun);
        assert_ne!(TaskStatus::NotApplicable, TaskStatus::Ok);
    }

    #[test]
    fn task_entry_clone() {
        let entry = TaskEntry {
            name: "test-task".to_string(),
            status: TaskStatus::Ok,
            message: Some("all good".to_string()),
            actions: ActionCounts::default(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.name, entry.name);
        assert_eq!(cloned.status, entry.status);
        assert_eq!(cloned.message, entry.message);
        assert_eq!(cloned.actions, entry.actions);
    }

    #[test]
    fn action_counts_merge_saturates() {
        let mut counts = ActionCounts {
            applied: u32::MAX,
            planned: 1,
            skipped: 2,
            failed: 3,
        };
        counts.merge(ActionCounts {
            applied: 1,
            planned: 4,
            skipped: 5,
            failed: 6,
        });

        assert_eq!(
            counts,
            ActionCounts {
                applied: u32::MAX,
                planned: 5,
                skipped: 7,
                failed: 9,
            }
        );
    }
}
