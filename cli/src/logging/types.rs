//! Core logging types: task entries, status, and the [`Log`] trait.
use super::diagnostic::DiagnosticLog;

/// Task execution result for summary reporting.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// Human-readable task name.
    pub name: String,
    /// Final status of the task.
    pub status: TaskStatus,
    /// Optional detail message (e.g., skip reason or error description).
    pub message: Option<String>,
}

/// Status of a completed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task completed successfully.
    Ok,
    /// Task was skipped because it does not apply to the current platform or profile.
    NotApplicable,
    /// Task was explicitly skipped (e.g., tool not found, config empty).
    Skipped,
    /// Task ran in dry-run mode; no changes were applied.
    DryRun,
    /// Task encountered an error and could not complete.
    Failed,
}

/// Abstraction over logging backends.
///
/// Both [`Logger`](super::logger::Logger) (direct output) and
/// [`BufferedLog`](super::buffered::BufferedLog) (deferred output for
/// parallel tasks) implement this trait, allowing task code to log without
/// knowing whether output is immediate or buffered.
pub trait Log: Send + Sync {
    /// Log a stage header (major section).
    fn stage(&self, msg: &str);
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
    /// Record a task result for the summary.
    fn record_task(&self, name: &str, status: TaskStatus, message: Option<&str>);
    /// Access the high-precision diagnostic log, if available.
    fn diagnostic(&self) -> Option<&DiagnosticLog> {
        None
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn task_status_equality() {
        assert_eq!(TaskStatus::Ok, TaskStatus::Ok);
        assert_eq!(TaskStatus::Failed, TaskStatus::Failed);
        assert_ne!(TaskStatus::Ok, TaskStatus::Failed);
        assert_ne!(TaskStatus::Skipped, TaskStatus::DryRun);
        assert_ne!(TaskStatus::NotApplicable, TaskStatus::Ok);
    }

    #[test]
    fn task_entry_clone() {
        let entry = TaskEntry {
            name: "test-task".to_string(),
            status: TaskStatus::Ok,
            message: Some("all good".to_string()),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.name, entry.name);
        assert_eq!(cloned.status, entry.status);
        assert_eq!(cloned.message, entry.message);
    }
}
