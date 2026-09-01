//! Task execution engine: applicability evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it decides applicability, runs the task,
//! and records the outcome into the logger.

use crate::engine::{Context, TaskResult};
use crate::infra::exec::ExecError;
use crate::infra::logging::{
    ActionCounts, LogEvent, TaskEntry, TaskStatus, format_elapsed, log_task_context,
};

use super::{Task, TaskAssessment};
use crate::infra::logging::OutputExt as _;

/// Dependency meaning of a task's recorded result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDisposition {
    Satisfied,
    Unmet,
    Failed,
    Cancelled,
}

/// Recorded presentation status plus dependency semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskExecution {
    pub(crate) status: TaskStatus,
    pub(crate) disposition: TaskDisposition,
}

impl TaskExecution {
    const fn new(status: TaskStatus, disposition: TaskDisposition) -> Self {
        Self {
            status,
            disposition,
        }
    }
}

/// Record a task that does not apply to this run.
///
/// `reason` is `None` when applicability was decided by the task's own
/// `should_run` check and there is nothing more specific to say; the `N/A`
/// status is the whole explanation in that case, so no reason is invented.
fn record_not_applicable(ctx: &Context, task: &dyn Task, task_id: &str, reason: Option<&str>) {
    let event_detail = reason.unwrap_or("not applicable");
    ctx.log()
        .run_task_event(LogEvent::TaskSkip, task.name(), event_detail);
    ctx.log().record_task(TaskEntry::new(
        task_id,
        task.name(),
        TaskStatus::NotApplicable,
        reason,
        ActionCounts::default(),
        task.visibility(),
    ));
}

/// Execute a task, recording the result in the logger.
///
/// Each task invocation is wrapped in a [`tracing::info_span`] so that
/// the log file and diagnostic output include structured context about
/// which task produced each message.
///
/// A typed executor cancellation error is recorded as [`TaskStatus::Skipped`]
/// with an "interrupted" message. Other failures remain failures even if the
/// global cancellation token was requested independently.
///
/// Every executed task also emits a [`LogEvent::TaskTiming`] entry recording
/// how long it ran. Under parallel scheduling the interleaved run log cannot
/// be used to infer per-task duration by subtracting timestamps, so the
/// measurement is taken here.
pub fn execute(task: &dyn Task, ctx: &Context) -> TaskStatus {
    let assessment = task.assess(ctx);
    execute_assessed(task, &assessment, ctx).status
}

/// Execute using the assessment precomputed by the application coordinator.
pub(crate) fn execute_assessed(
    task: &dyn Task,
    assessment: &TaskAssessment,
    ctx: &Context,
) -> TaskExecution {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    let _diag_context = log_task_context(task.name());
    let task_id = task.task_id().record_key();
    if !assessment.is_applicable() {
        record_not_applicable(ctx, task, &task_id, assessment.not_applicable_reason());
        return TaskExecution::new(TaskStatus::NotApplicable, TaskDisposition::Satisfied);
    }

    ctx.log()
        .run_task_event(LogEvent::TaskStart, task.name(), "executing");
    let started = std::time::Instant::now();
    let execution = record_run_outcome(task, &task_id, ctx);
    let elapsed = started.elapsed();
    ctx.log().record_task_duration(&task_id, elapsed);
    ctx.log().run_task_event(
        LogEvent::TaskTiming,
        task.name(),
        &format!("elapsed {}", format_elapsed(elapsed)),
    );
    execution
}

/// Record a task outcome with the task's own visibility, returning the status.
fn record(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    status: TaskStatus,
    message: Option<&str>,
    actions: ActionCounts,
) -> TaskStatus {
    ctx.log().record_task(TaskEntry::new(
        task_id,
        task.name(),
        status,
        message,
        actions,
        task.visibility(),
    ));
    status
}

/// Downgrade a cancellation-induced failure to [`TaskStatus::Skipped`].
///
/// Ctrl-C aborts in-flight work, so the resulting errors are signal artefacts
/// rather than real failures and must not be counted as such in the summary.
fn record_interrupted(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    detail: &str,
    actions: ActionCounts,
) -> TaskStatus {
    ctx.log()
        .run_task_event(LogEvent::TaskSkip, task.name(), "interrupted");
    ctx.log().warn(format!("interrupted: {detail}"));
    record(
        task,
        task_id,
        ctx,
        TaskStatus::Skipped,
        Some("interrupted"),
        actions,
    )
}

/// Run a task and record its outcome.
///
/// Typed executor cancellation errors are downgraded to [`TaskStatus::Skipped`]
/// so the summary does not count signal interruptions as real failures.
fn record_run_outcome(task: &dyn Task, task_id: &str, ctx: &Context) -> TaskExecution {
    let rec = |status: TaskStatus, msg: Option<&str>| {
        record(task, task_id, ctx, status, msg, ActionCounts::default())
    };
    match task.run_configured(ctx) {
        Ok(None) => {
            ctx.log()
                .run_task_event(LogEvent::TaskSkip, task.name(), "nothing configured");
            TaskExecution::new(
                rec(TaskStatus::NotApplicable, Some("nothing configured")),
                TaskDisposition::Satisfied,
            )
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "ok");
                TaskExecution::new(rec(TaskStatus::Ok, None), TaskDisposition::Satisfied)
            }
            TaskResult::DryRun => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "planned");
                TaskExecution::new(rec(TaskStatus::DryRun, None), TaskDisposition::Satisfied)
            }
            TaskResult::CheckPassed => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "passed");
                TaskExecution::new(rec(TaskStatus::Passed, None), TaskDisposition::Satisfied)
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log()
                    .run_task_event(LogEvent::TaskSkip, task.name(), &reason);
                TaskExecution::new(
                    rec(TaskStatus::NotApplicable, Some(&reason)),
                    TaskDisposition::Satisfied,
                )
            }
            TaskResult::Skipped { reason, kind } => {
                if kind.is_failure() && ctx.require_complete() {
                    return TaskExecution::new(
                        record_failed_outcome(task, task_id, ctx, &reason),
                        TaskDisposition::Unmet,
                    );
                }
                ctx.log()
                    .run_task_event(LogEvent::TaskSkip, task.name(), &reason);
                TaskExecution::new(
                    rec(TaskStatus::Skipped, Some(&reason)),
                    if kind.is_failure() {
                        TaskDisposition::Unmet
                    } else {
                        TaskDisposition::Satisfied
                    },
                )
            }
            TaskResult::Failed(reason) => TaskExecution::new(
                record_failed_outcome(task, task_id, ctx, &reason),
                TaskDisposition::Failed,
            ),
            TaskResult::Batch(stats) => {
                let batch_status = record_batch_outcome(task, task_id, ctx, &stats);
                TaskExecution::new(
                    batch_status,
                    if batch_status == TaskStatus::Failed {
                        TaskDisposition::Failed
                    } else {
                        TaskDisposition::Satisfied
                    },
                )
            }
        },
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
                    || cause
                        .downcast_ref::<crate::engine::resource::ResourceError>()
                        .is_some_and(crate::engine::resource::ResourceError::is_cancelled)
            }) =>
        {
            TaskExecution::new(
                record_interrupted(task, task_id, ctx, task.name(), ActionCounts::default()),
                TaskDisposition::Cancelled,
            )
        }
        Err(e) => {
            let message = format!("{e:#}");
            ctx.log()
                .run_task_event(LogEvent::TaskFail, task.name(), &message);
            ctx.log().error(format!("{}: {message}", task.name()));
            TaskExecution::new(
                rec(TaskStatus::Failed, Some(&message)),
                TaskDisposition::Failed,
            )
        }
    }
}

fn record_failed_outcome(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    reason: &str,
) -> TaskStatus {
    let actions = ActionCounts {
        failed: 1,
        ..ActionCounts::default()
    };
    ctx.log()
        .run_task_event(LogEvent::TaskFail, task.name(), reason);
    ctx.log().warn(format!("failed: {reason}"));
    record(
        task,
        task_id,
        ctx,
        TaskStatus::Failed,
        Some(reason),
        actions,
    )
}

fn record_batch_outcome(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    stats: &crate::engine::TaskStats,
) -> TaskStatus {
    let message = stats
        .message()
        .map_or_else(|| stats.summary(ctx.dry_run()), str::to_string);
    let actions = ActionCounts {
        applied: if ctx.dry_run() {
            0
        } else {
            stats.changed_count()
        },
        planned: if ctx.dry_run() {
            stats.changed_count()
        } else {
            0
        },
        skipped: stats.skipped_count(),
        failed: stats.failed_count(),
    };
    let outcome = if stats.failed_count() > 0 {
        TaskStatus::Failed
    } else if ctx.dry_run() && stats.changed_count() > 0 {
        TaskStatus::DryRun
    } else if stats.changed_count() > 0 {
        TaskStatus::Changed
    } else if stats.skipped_count() > 0 {
        TaskStatus::Skipped
    } else {
        TaskStatus::Ok
    };

    let event = if outcome == TaskStatus::Failed {
        LogEvent::TaskFail
    } else {
        LogEvent::TaskDone
    };
    ctx.log().run_task_event(event, task.name(), &message);
    if outcome == TaskStatus::Failed {
        ctx.log().warn(format!("failed: {message}"));
    } else {
        ctx.log().info(&message);
    }
    let recorded_message = batch_reason(stats, outcome, &message);
    record(
        task,
        task_id,
        ctx,
        outcome,
        recorded_message.as_deref(),
        actions,
    )
}

/// The reason shown on a batch task's status row.
///
/// A skipped batch always states why it did nothing: the aggregate counters are
/// the only reason available once per-item detail has been filtered out, and a
/// bare `IGNORE` row is the outcome users most often have to ask about.
fn batch_reason(
    stats: &crate::engine::TaskStats,
    outcome: TaskStatus,
    message: &str,
) -> Option<String> {
    if let Some(custom) = stats.message() {
        return Some(custom.to_string());
    }
    match outcome {
        TaskStatus::Changed | TaskStatus::Failed => Some(message.to_string()),
        TaskStatus::Skipped => Some(format!(
            "{} {} skipped",
            stats.skipped_count(),
            if stats.skipped_count() == 1 {
                "item"
            } else {
                "items"
            }
        )),
        TaskStatus::Ok | TaskStatus::Passed | TaskStatus::DryRun | TaskStatus::NotApplicable => {
            None
        }
    }
}
