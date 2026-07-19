//! Task execution engine: applicability evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it decides applicability, runs the task,
//! and records the outcome into the logger.

use crate::engine::{Context, TaskResult};
use crate::infra::logging::{ActionCounts, DiagEvent, TaskStatus, diag_task_context};

use super::{Task, TaskPhase};

pub(super) fn not_applicable_reason<T: Task + ?Sized>(task: &T, ctx: &Context) -> Option<String> {
    if task.should_run(ctx) {
        None
    } else {
        Some("not applicable".to_string())
    }
}

fn record_not_applicable(ctx: &Context, name: &str, reason: &str) {
    ctx.log().diag_task(DiagEvent::TaskSkip, name, reason);
    ctx.debug_fmt(|| format!("not applicable: {reason}"));
    ctx.log().record_task(name, TaskStatus::NotApplicable, None);
}

/// Execute a task, recording the result in the logger.
///
/// Each task invocation is wrapped in a [`tracing::info_span`] so that
/// the log file and diagnostic output include structured context about
/// which task produced each message.
///
/// If cancellation has been requested (Ctrl-C) and a task returns
/// [`TaskResult::Failed`] or an error, the failure is downgraded to
/// [`TaskStatus::Skipped`] with an "interrupted" message so the
/// summary does not count signal-induced failures.
pub fn execute(task: &dyn Task, ctx: &Context) -> TaskStatus {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    let _diag_context = diag_task_context(task.name());
    if let Some(reason) = not_applicable_reason(task, ctx) {
        record_not_applicable(ctx, task.name(), &reason);
        return TaskStatus::NotApplicable;
    }

    ctx.log()
        .diag_task(DiagEvent::TaskStart, task.name(), "executing");
    record_run_outcome(task, ctx)
}

/// Run a task and record its outcome.
///
/// Cancellation-induced failures (Ctrl-C) are downgraded to
/// [`TaskStatus::Skipped`] so the summary does not count signal
/// interruptions as real failures.
fn record_run_outcome(task: &dyn Task, ctx: &Context) -> TaskStatus {
    let rec = |status: TaskStatus, msg: Option<&str>, actions: ActionCounts| {
        ctx.log()
            .record_task_with_actions(task.name(), status, msg, actions);
        status
    };
    match task.run_configured(ctx) {
        Ok(None) => {
            ctx.log()
                .diag_task(DiagEvent::TaskSkip, task.name(), "nothing configured");
            ctx.log().debug("nothing configured");
            ctx.log()
                .record_task(task.name(), TaskStatus::NotApplicable, None);
            TaskStatus::NotApplicable
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log().diag_task(DiagEvent::TaskDone, task.name(), "ok");
                let status = if task.phase() == TaskPhase::Validation {
                    TaskStatus::Changed
                } else {
                    TaskStatus::Ok
                };
                rec(status, None, ActionCounts::default())
            }
            TaskResult::OkWithMessage(message) => {
                ctx.log()
                    .diag_task(DiagEvent::TaskDone, task.name(), &message);
                ctx.log().info(&message);
                rec(
                    TaskStatus::Changed,
                    Some(&message),
                    ActionCounts {
                        applied: 1,
                        ..ActionCounts::default()
                    },
                )
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log()
                    .diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.debug_fmt(|| format!("not applicable: {reason}"));
                ctx.log()
                    .record_task(task.name(), TaskStatus::NotApplicable, None);
                TaskStatus::NotApplicable
            }
            TaskResult::Skipped(reason) => {
                ctx.log()
                    .diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.log().info(&reason);
                rec(TaskStatus::Skipped, Some(&reason), ActionCounts::default())
            }
            TaskResult::Failed(reason) => record_failed_outcome(task, ctx, &reason),
            TaskResult::DryRun => {
                ctx.log()
                    .diag_task(DiagEvent::TaskDone, task.name(), "dry-run");
                rec(
                    TaskStatus::DryRun,
                    None,
                    ActionCounts {
                        planned: 1,
                        ..ActionCounts::default()
                    },
                )
            }
            TaskResult::Batch(stats) => record_batch_outcome(task, ctx, &stats),
        },
        Err(e) => {
            if ctx.is_cancelled() {
                ctx.log()
                    .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                ctx.log().warn(&format!("interrupted: {}", task.name()));
                rec(
                    TaskStatus::Skipped,
                    Some("interrupted"),
                    ActionCounts::default(),
                )
            } else {
                ctx.log()
                    .diag_task(DiagEvent::TaskFail, task.name(), &format!("{e:#}"));
                ctx.log().error(&format!("{}: {e:#}", task.name()));
                rec(
                    TaskStatus::Failed,
                    Some(&format!("{e:#}")),
                    ActionCounts::default(),
                )
            }
        }
    }
}

fn record_failed_outcome(task: &dyn Task, ctx: &Context, reason: &str) -> TaskStatus {
    let actions = ActionCounts {
        failed: 1,
        ..ActionCounts::default()
    };
    if ctx.is_cancelled() {
        ctx.log()
            .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
        ctx.log().warn(&format!("interrupted: {reason}"));
        ctx.log().record_task_with_actions(
            task.name(),
            TaskStatus::Skipped,
            Some("interrupted"),
            actions,
        );
        TaskStatus::Skipped
    } else {
        ctx.log()
            .diag_task(DiagEvent::TaskFail, task.name(), reason);
        ctx.log().warn(&format!("failed: {reason}"));
        ctx.log()
            .record_task_with_actions(task.name(), TaskStatus::Failed, Some(reason), actions);
        TaskStatus::Failed
    }
}

fn record_batch_outcome(
    task: &dyn Task,
    ctx: &Context,
    stats: &crate::engine::TaskStats,
) -> TaskStatus {
    let message = stats.summary(ctx.dry_run());
    let actions = ActionCounts {
        applied: if ctx.dry_run() { 0 } else { stats.changed },
        planned: if ctx.dry_run() { stats.changed } else { 0 },
        skipped: stats.skipped,
        failed: stats.failed,
    };
    let outcome = if stats.failed > 0 {
        TaskStatus::Failed
    } else if ctx.dry_run() && stats.changed > 0 {
        TaskStatus::DryRun
    } else if stats.changed > 0 {
        TaskStatus::Changed
    } else if stats.skipped > 0 {
        TaskStatus::Skipped
    } else {
        TaskStatus::Ok
    };

    if outcome == TaskStatus::Failed && ctx.is_cancelled() {
        ctx.log()
            .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
        ctx.log().warn(&format!("interrupted: {message}"));
        ctx.log().record_task_with_actions(
            task.name(),
            TaskStatus::Skipped,
            Some("interrupted"),
            actions,
        );
        return TaskStatus::Skipped;
    }

    let event = if outcome == TaskStatus::Failed {
        DiagEvent::TaskFail
    } else {
        DiagEvent::TaskDone
    };
    ctx.log().diag_task(event, task.name(), &message);
    if outcome == TaskStatus::Failed {
        ctx.log().warn(&format!("failed: {message}"));
    }
    let recorded_message =
        matches!(outcome, TaskStatus::Changed | TaskStatus::Failed).then_some(message.as_str());
    ctx.log()
        .record_task_with_actions(task.name(), outcome, recorded_message, actions);
    outcome
}
