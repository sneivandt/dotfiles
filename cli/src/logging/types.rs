//! Core logging types: task entries, status, and the [`Log`] trait.
use super::diagnostic::{DiagEvent, DiagnosticLog};
use crate::tasks::Domain;

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
    /// Icon and ANSI color used for compact status rendering.
    #[must_use]
    pub const fn icon_and_color(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Changed | Self::Ok => Some(("\u{25cf}", "\x1b[32m")),
            Self::Skipped => Some(("\u{25cb}", "\x1b[33m")),
            Self::DryRun => Some(("\u{25cb}", "\x1b[35m")),
            Self::Failed => Some(("\u{2717}", "\x1b[31m")),
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
/// Both [`Logger`](super::runtime::Logger) and
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
    /// Emit a stage header only when debug logging is active.
    fn debug_stage(&self, msg: &str) {
        if self.debug_enabled() {
            self.stage(msg);
        }
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
    fn record_task(&self, name: &str, domain: Domain, status: TaskStatus, message: Option<&str>);
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
pub trait Log: Output + TaskRecorder {
    /// Record a task outcome for the final grouped summary.
    fn record_task_outcome(
        &self,
        name: &str,
        domain: Domain,
        status: TaskStatus,
        message: Option<&str>,
    ) {
        self.record_task(name, domain, status, message);
    }
}

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
    use crate::tasks::TaskPhase;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingLog {
        records: AtomicUsize,
    }

    macro_rules! no_op_output_methods {
        ($($method:ident),+ $(,)?) => {
            $(
                fn $method(&self, _msg: &str) {}
            )+
        };
    }

    impl Output for RecordingLog {
        no_op_output_methods!(stage, info, debug, warn, error, dry_run, always);
    }

    impl TaskRecorder for RecordingLog {
        fn record_task(
            &self,
            _name: &str,
            _domain: Domain,
            _status: TaskStatus,
            _message: Option<&str>,
        ) {
            self.records.fetch_add(1, Ordering::SeqCst);
        }
    }

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
    fn record_task_outcome_does_not_emit_inline_task_result() {
        let log = RecordingLog::default();

        log.record_task_outcome(
            "skipped-task",
            Domain::General,
            TaskStatus::Skipped,
            Some("not needed"),
        );

        assert_eq!(log.records.load(Ordering::SeqCst), 1);
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

    #[test]
    fn task_phase_display() {
        assert_eq!(TaskPhase::Bootstrap.to_string(), "Bootstrap");
        assert_eq!(TaskPhase::Sync.to_string(), "Sync");
        assert_eq!(TaskPhase::Provision.to_string(), "Provision");
        assert_eq!(TaskPhase::Validation.to_string(), "Validation");
        assert_eq!(TaskPhase::Update.to_string(), "Update");
    }

    #[test]
    fn task_phase_labels() {
        assert_eq!(TaskPhase::Bootstrap.label(), "Setting up dotfiles");
        assert_eq!(TaskPhase::Sync.label(), "Updating the repository");
        assert_eq!(TaskPhase::Provision.label(), "Configuring your system");
        assert_eq!(TaskPhase::Validation.label(), "Checking the setup");
        assert_eq!(TaskPhase::Update.label(), "Updating dependencies");
    }

    #[test]
    fn task_phase_equality() {
        assert_eq!(TaskPhase::Bootstrap, TaskPhase::Bootstrap);
        assert_eq!(TaskPhase::Sync, TaskPhase::Sync);
        assert_eq!(TaskPhase::Provision, TaskPhase::Provision);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Sync);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Provision);
        assert_ne!(TaskPhase::Sync, TaskPhase::Provision);
    }
}
