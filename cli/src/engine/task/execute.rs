//! Task execution engine: applicability evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it decides applicability, runs the task,
//! and records the outcome into the logger.

use crate::engine::{Context, TaskResult, TaskVisibility};
use crate::infra::logging::{ActionCounts, LogEvent, TaskStatus, format_elapsed, log_task_context};

use super::Task;
use crate::infra::logging::OutputExt as _;

/// Record a task that does not apply to this run.
///
/// `reason` is `None` when applicability was decided by the task's own
/// `should_run` check and there is nothing more specific to say; the `N/A`
/// status is the whole explanation in that case, so no reason is invented.
fn record_not_applicable(ctx: &Context, task: &dyn Task, reason: Option<&str>) {
    let event_detail = reason.unwrap_or("not applicable");
    ctx.log()
        .run_task_event(LogEvent::TaskSkip, task.name(), event_detail);
    ctx.log().record_task_with_metadata(
        task.name(),
        TaskStatus::NotApplicable,
        reason,
        ActionCounts::default(),
        task.visibility() == TaskVisibility::Visible,
    );
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
///
/// Every executed task also emits a [`LogEvent::TaskTiming`] entry recording
/// how long it ran. Under parallel scheduling the interleaved run log cannot
/// be used to infer per-task duration by subtracting timestamps, so the
/// measurement is taken here.
pub fn execute(task: &dyn Task, ctx: &Context) -> TaskStatus {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    let _diag_context = log_task_context(task.name());
    if !task.should_run(ctx) {
        record_not_applicable(ctx, task, None);
        return TaskStatus::NotApplicable;
    }

    ctx.log()
        .run_task_event(LogEvent::TaskStart, task.name(), "executing");
    let started = std::time::Instant::now();
    let status = record_run_outcome(task, ctx);
    let elapsed = started.elapsed();
    ctx.log().record_task_duration(task.name(), elapsed);
    ctx.log().run_task_event(
        LogEvent::TaskTiming,
        task.name(),
        &format!("elapsed {}", format_elapsed(elapsed)),
    );
    status
}

/// Record a task outcome with the task's own visibility, returning the status.
fn record(
    task: &dyn Task,
    ctx: &Context,
    status: TaskStatus,
    message: Option<&str>,
    actions: ActionCounts,
) -> TaskStatus {
    ctx.log().record_task_with_metadata(
        task.name(),
        status,
        message,
        actions,
        task.visibility() == TaskVisibility::Visible,
    );
    status
}

/// Downgrade a cancellation-induced failure to [`TaskStatus::Skipped`].
///
/// Ctrl-C aborts in-flight work, so the resulting errors are signal artefacts
/// rather than real failures and must not be counted as such in the summary.
fn record_interrupted(
    task: &dyn Task,
    ctx: &Context,
    detail: &str,
    actions: ActionCounts,
) -> TaskStatus {
    ctx.log()
        .run_task_event(LogEvent::TaskSkip, task.name(), "interrupted");
    ctx.log().warn(format!("interrupted: {detail}"));
    record(task, ctx, TaskStatus::Skipped, Some("interrupted"), actions)
}

/// Run a task and record its outcome.
///
/// Cancellation-induced failures (Ctrl-C) are downgraded to
/// [`TaskStatus::Skipped`] so the summary does not count signal
/// interruptions as real failures.
fn record_run_outcome(task: &dyn Task, ctx: &Context) -> TaskStatus {
    let rec = |status: TaskStatus, msg: Option<&str>| {
        record(task, ctx, status, msg, ActionCounts::default())
    };
    match task.run_configured(ctx) {
        Ok(None) => {
            ctx.log()
                .run_task_event(LogEvent::TaskSkip, task.name(), "nothing configured");
            rec(TaskStatus::NotApplicable, Some("nothing configured"))
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "ok");
                rec(TaskStatus::Ok, None)
            }
            TaskResult::DryRun => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "planned");
                rec(TaskStatus::DryRun, None)
            }
            TaskResult::CheckPassed => {
                ctx.log()
                    .run_task_event(LogEvent::TaskDone, task.name(), "passed");
                rec(TaskStatus::Passed, None)
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log()
                    .run_task_event(LogEvent::TaskSkip, task.name(), &reason);
                rec(TaskStatus::NotApplicable, Some(&reason))
            }
            TaskResult::Skipped(reason) => {
                ctx.log()
                    .run_task_event(LogEvent::TaskSkip, task.name(), &reason);
                rec(TaskStatus::Skipped, Some(&reason))
            }
            TaskResult::Failed(reason) => record_failed_outcome(task, ctx, &reason),
            TaskResult::Batch(stats) => record_batch_outcome(task, ctx, &stats),
        },
        Err(_) if ctx.is_cancelled() => {
            record_interrupted(task, ctx, task.name(), ActionCounts::default())
        }
        Err(e) => {
            let message = format!("{e:#}");
            ctx.log()
                .run_task_event(LogEvent::TaskFail, task.name(), &message);
            ctx.log().error(format!("{}: {message}", task.name()));
            rec(TaskStatus::Failed, Some(&message))
        }
    }
}

fn record_failed_outcome(task: &dyn Task, ctx: &Context, reason: &str) -> TaskStatus {
    let actions = ActionCounts {
        failed: 1,
        ..ActionCounts::default()
    };
    if ctx.is_cancelled() {
        return record_interrupted(task, ctx, reason, actions);
    }
    ctx.log()
        .run_task_event(LogEvent::TaskFail, task.name(), reason);
    ctx.log().warn(format!("failed: {reason}"));
    record(task, ctx, TaskStatus::Failed, Some(reason), actions)
}

fn record_batch_outcome(
    task: &dyn Task,
    ctx: &Context,
    stats: &crate::engine::TaskStats,
) -> TaskStatus {
    let message = stats
        .message
        .clone()
        .unwrap_or_else(|| stats.summary(ctx.dry_run()));
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
        return record_interrupted(task, ctx, &message, actions);
    }

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
    record(task, ctx, outcome, recorded_message.as_deref(), actions)
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
    if let Some(custom) = stats.message.clone() {
        return Some(custom);
    }
    match outcome {
        TaskStatus::Changed | TaskStatus::Failed => Some(message.to_string()),
        TaskStatus::Skipped => Some(format!(
            "{} {} skipped",
            stats.skipped,
            if stats.skipped == 1 { "item" } else { "items" }
        )),
        TaskStatus::Ok | TaskStatus::Passed | TaskStatus::DryRun | TaskStatus::NotApplicable => {
            None
        }
    }
}
