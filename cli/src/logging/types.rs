//! Core logging types: task entries, status, and the [`Log`] trait.
use super::diagnostic::DiagnosticLog;
use crate::tasks::TaskPhase;

/// Task execution result for summary reporting.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// Human-readable task name.
    pub name: String,
    /// Execution phase of the task.
    pub phase: TaskPhase,
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
    /// Log a compact task-result line (console-only, omitted from the log file).
    fn task_result(&self, msg: &str);
    /// Whether verbose output mode is enabled.
    ///
    /// When `false`, stage headers and plain info messages are suppressed on
    /// the console and replaced by compact inline task-result lines.
    fn is_verbose(&self) -> bool {
        true
    }
    /// Emit a compact inline task-result line.
    ///
    /// Default implementation formats icon + name + optional detail and
    /// routes through [`always`](Self::always).  `NotApplicable` tasks are
    /// silently ignored.
    fn emit_task_result(&self, name: &str, status: &TaskStatus, message: Option<&str>) {
        let (icon, color) = match status {
            TaskStatus::Ok => ("\u{2713}", "\x1b[32m"),
            TaskStatus::Skipped => ("\u{25cb}", "\x1b[33m"),
            TaskStatus::Failed => ("\u{2717}", "\x1b[31m"),
            TaskStatus::DryRun => ("~", "\x1b[35m"),
            TaskStatus::NotApplicable => return,
        };
        let suffix = message.map_or_else(String::new, |msg| format!(" \u{2014} {msg}"));
        self.task_result(&format!("{color}  {icon} {name}{suffix}\x1b[0m"));
    }
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
}

/// Structured task result recording for summary reports.
///
/// Separated from [`Output`] so that resource-processing code can depend
/// only on display methods while the scheduler records task outcomes
/// independently.
pub trait TaskRecorder: Send + Sync {
    /// Record a task result for the summary.
    fn record_task(&self, name: &str, phase: TaskPhase, status: TaskStatus, message: Option<&str>);
}

/// Combined logging interface: user-facing output plus task recording.
///
/// This is the primary trait stored in [`Context`](crate::engine::Context).
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
            phase: TaskPhase::Apply,
            status: TaskStatus::Ok,
            message: Some("all good".to_string()),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.name, entry.name);
        assert_eq!(cloned.phase, entry.phase);
        assert_eq!(cloned.status, entry.status);
        assert_eq!(cloned.message, entry.message);
    }

    #[test]
    fn task_phase_display() {
        assert_eq!(TaskPhase::Bootstrap.to_string(), "Bootstrap");
        assert_eq!(TaskPhase::Repository.to_string(), "Repository");
        assert_eq!(TaskPhase::Apply.to_string(), "Apply");
    }

    #[test]
    fn task_phase_equality() {
        assert_eq!(TaskPhase::Bootstrap, TaskPhase::Bootstrap);
        assert_eq!(TaskPhase::Repository, TaskPhase::Repository);
        assert_eq!(TaskPhase::Apply, TaskPhase::Apply);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Repository);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Apply);
        assert_ne!(TaskPhase::Repository, TaskPhase::Apply);
    }
}
