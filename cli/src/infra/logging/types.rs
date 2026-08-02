//! Core logging types: task entries, status, and the [`Log`] trait.
use std::borrow::Cow;

use super::runlog::{LogEvent, RunLog};
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
}

/// Whether a task is part of the user-facing workflow or internal orchestration.
///
/// Lives here rather than beside the task contract because it is purely a
/// presentation decision: it selects which rows and totals a run reports. The
/// engine re-exports it so task implementations need not reach into the
/// logging module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskVisibility {
    /// Show the task in discovery, completed rows, and aggregate totals.
    Visible,
    /// Keep the task in scheduling and diagnostics, but hide it from normal output.
    Internal,
}

impl TaskVisibility {
    /// Whether this task contributes to user-facing rows and totals.
    #[must_use]
    pub const fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
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
    /// Whether the task contributes to user-facing rows and totals.
    pub visibility: TaskVisibility,
    /// How long the task ran, once measured by the execution engine.
    ///
    /// Recorded separately from the outcome because the duration is only known
    /// after the task body returns. Surfaced on verbose status rows.
    pub duration: Option<std::time::Duration>,
}

/// Status of a completed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task completed successfully and changed system state.
    Changed,
    /// Task completed successfully without a recorded state change.
    Ok,
    /// A validation check passed.
    Passed,
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
    pub(in crate::infra::logging) const fn text_style(self) -> TextStyle {
        match self {
            Self::Changed | Self::Passed => TextStyle::Green,
            Self::Ok | Self::NotApplicable => TextStyle::Dim,
            Self::Skipped => TextStyle::Yellow,
            Self::DryRun => TextStyle::Magenta,
            Self::Failed => TextStyle::Red,
        }
    }
}

/// The kind of a user-facing message.
///
/// This is the single axis along which display output varies: it selects the
/// run-log event kind, the console tracing target, and how the message is
/// replayed after a parallel task completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    /// A stage header (major section).
    Stage,
    /// A task name, without major-section emphasis.
    TaskStage,
    /// An informational message.
    Info,
    /// A debug message; never rendered on the console.
    Debug,
    /// A warning message.
    Warn,
    /// An error message.
    Error,
    /// A dry-run action message.
    DryRun,
    /// A message that appears on the console regardless of the verbose setting.
    ///
    /// Used for structural output such as the version banner and summary.
    Always,
    /// The startup context header, rendered dim on the console.
    ///
    /// Always visible, like [`MsgKind::Always`], but de-emphasised because it
    /// describes the run rather than reporting its results.
    Startup,
}

impl MsgKind {
    /// The run-log event kind recorded for this message.
    #[must_use]
    pub(in crate::infra::logging) const fn log_event(self) -> LogEvent {
        match self {
            Self::Stage | Self::TaskStage => LogEvent::Stage,
            Self::Info | Self::Always | Self::Startup => LogEvent::Info,
            Self::Debug => LogEvent::Debug,
            Self::Warn => LogEvent::Warn,
            Self::Error => LogEvent::Error,
            Self::DryRun => LogEvent::DryRun,
        }
    }
}

/// Emit `$msg` to the console tracing target that matches `$kind`.
///
/// Tracing requires a literal target, so the mapping cannot be expressed as a
/// function; this macro keeps [`Logger`](super::logger::Logger) and buffered
/// replay using exactly the same targets.
macro_rules! emit_console_event {
    ($kind:expr, $msg:expr) => {{
        let msg = $msg;
        match $kind {
            $crate::infra::logging::MsgKind::Stage => {
                tracing::info!(target: "dotfiles::ui::stage", "{msg}");
            }
            $crate::infra::logging::MsgKind::TaskStage => {
                tracing::info!(target: "dotfiles::ui::task_stage", "{msg}");
            }
            $crate::infra::logging::MsgKind::Info => {
                tracing::info!(target: "dotfiles::ui::info", "{msg}");
            }
            $crate::infra::logging::MsgKind::Debug => {
                tracing::debug!(target: "dotfiles::ui::debug", "{msg}");
            }
            $crate::infra::logging::MsgKind::Warn => {
                tracing::warn!(target: "dotfiles::ui::warn", "{msg}");
            }
            $crate::infra::logging::MsgKind::Error => {
                tracing::error!(target: "dotfiles::ui::error", "{msg}");
            }
            $crate::infra::logging::MsgKind::DryRun => {
                tracing::info!(target: "dotfiles::ui::dry_run", "{msg}");
            }
            $crate::infra::logging::MsgKind::Always => {
                tracing::info!(target: "dotfiles::ui::always", "{msg}");
            }
            $crate::infra::logging::MsgKind::Startup => {
                tracing::info!(target: "dotfiles::ui::startup", "{msg}");
            }
        }
    }};
}

pub(in crate::infra::logging) use emit_console_event;

/// User-facing output sink.
///
/// This trait covers display-oriented logging: stage headers, informational
/// messages, debug output, warnings, errors, and dry-run annotations. It
/// intentionally excludes structured task recording, which belongs to
/// [`TaskRecorder`].
///
/// Implementors provide a single [`emit`](Output::emit) method; the named
/// helpers (`info`, `warn`, …) live on [`OutputExt`], which is blanket
/// implemented for every `Output`.
///
/// Both [`Logger`](super::logger::Logger) and
/// [`BufferedLog`](super::buffered::BufferedLog) implement this trait.
pub trait Output: Send + Sync {
    /// Emit a user-facing message of the given kind.
    ///
    /// Taking a [`Cow`] lets callers that already built a `String` (via
    /// `format!`) hand over ownership instead of forcing a second copy when
    /// the message has to be buffered for later console replay.
    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>);

    /// Return whether debug logging is currently active on this thread.
    ///
    /// This intentionally avoids `tracing::enabled!`, which can leave stale
    /// per-layer filter state behind on replay paths.  The default
    /// implementation only checks whether a tracing dispatcher has been set,
    /// which is enough for this codebase because command execution installs a
    /// DEBUG-capable subscriber whenever logging is active.
    fn debug_enabled(&self) -> bool {
        tracing::dispatcher::has_been_set()
    }

    /// Access the run log, if one is available.
    fn run_log(&self) -> Option<&RunLog> {
        None
    }

    /// Emit a run-log event when the run log is enabled.
    ///
    /// This is a convenience wrapper around [`RunLog::emit`] that no-ops when
    /// no run log is configured, so call sites do not need to
    /// `if let Some(run_log) = ...` themselves.
    fn run_event(&self, event: LogEvent, message: &str) {
        if let Some(run_log) = self.run_log() {
            run_log.emit(event, message);
        }
    }

    /// Emit a task-scoped run-log event when the run log is enabled.
    ///
    /// Convenience wrapper around [`RunLog::emit_task`].
    fn run_task_event(&self, event: LogEvent, task: &str, message: &str) {
        if let Some(run_log) = self.run_log() {
            run_log.emit_task(event, task, message);
        }
    }
}

/// Named convenience methods for every [`Output`].
///
/// Each method accepts anything that converts into a `Cow<str>`, so both
/// `log.info("literal")` and `log.info(format!("{n} items"))` work, and the
/// latter moves its `String` rather than copying it.
///
/// Import it as `use crate::infra::logging::OutputExt as _;`.
pub trait OutputExt: Output {
    /// Log a stage header (major section).
    fn stage<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Stage, msg.into());
    }
    /// Log a task name without major-section emphasis.
    fn task_stage<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::TaskStage, msg.into());
    }
    /// Log an informational message.
    fn info<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Info, msg.into());
    }
    /// Log a debug message; recorded in the run log only.
    fn debug<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Debug, msg.into());
    }
    /// Log a warning message.
    fn warn<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Warn, msg.into());
    }
    /// Log an error message.
    fn error<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Error, msg.into());
    }
    /// Log a dry-run action message.
    fn dry_run<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::DryRun, msg.into());
    }
    /// Log a message that always appears on the console regardless of the
    /// verbose setting.  Used for structural output (version, profile, summary).
    fn always<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Always, msg.into());
    }
    /// Log the startup context header. Always visible, rendered dim.
    fn startup<'a>(&self, msg: impl Into<Cow<'a, str>>) {
        self.emit(MsgKind::Startup, msg.into());
    }
}

impl<T: Output + ?Sized> OutputExt for T {}

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

    /// Record a task result with presentation metadata.
    fn record_task_with_metadata(
        &self,
        name: &str,
        status: TaskStatus,
        message: Option<&str>,
        actions: ActionCounts,
        _visibility: TaskVisibility,
    ) {
        self.record_task_with_actions(name, status, message, actions);
    }

    /// Attach a measured run duration to an already-recorded task.
    ///
    /// Called after the task body returns, so it cannot be folded into the
    /// outcome-recording calls above. Recorders that do not display timing
    /// ignore it.
    fn record_task_duration(&self, _name: &str, _duration: std::time::Duration) {}
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
            visibility: TaskVisibility::Visible,
            duration: None,
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
