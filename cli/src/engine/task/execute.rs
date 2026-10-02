//! Task execution engine: applicability evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it decides applicability, runs the task,
//! and records the outcome into the logger.

use crate::engine::batch::is_interrupted;
use crate::engine::{BatchCompletion, BatchReport, Context, TaskResult, TaskStats};
use crate::infra::exec::ExecError;
use crate::infra::logging::{
    ActionCounts, LogEvent, TaskEntry, TaskStatus, format_elapsed, log_task_context,
};

use super::{Task, TaskAssessment};
use crate::infra::logging::OutputExt as _;

/// Dependency meaning of a task's recorded result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskOutcome {
    Satisfied,
    Unmet,
    Failed,
    Blocked,
    Cancelled,
}

/// Recorded presentation status plus dependency semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskExecution {
    pub(crate) status: TaskStatus,
    pub(crate) outcome: TaskOutcome,
}

impl TaskExecution {
    const fn new(status: TaskStatus, outcome: TaskOutcome) -> Self {
        Self { status, outcome }
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
        .run_task_event(LogEvent::TaskSkip, &task.log_key(), event_detail);
    record(
        task,
        task_id,
        ctx,
        TaskStatus::NotApplicable,
        reason,
        ActionCounts::default(),
        false,
    );
}

/// Execute a task, recording the result in the logger.
///
/// Each task invocation is wrapped in a [`tracing::info_span`] so that
/// the log file and diagnostic output include structured context about
/// which task produced each message.
///
/// A typed executor cancellation error is recorded as [`TaskStatus::Interrupted`]
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
    let _diag_context = log_task_context(&task.log_key());
    let task_id = task.log_key();
    if !assessment.is_applicable() {
        record_not_applicable(ctx, task, &task_id, assessment.not_applicable_reason());
        return TaskExecution::new(TaskStatus::NotApplicable, TaskOutcome::Satisfied);
    }

    ctx.log()
        .run_task_event(LogEvent::TaskStart, &task.log_key(), "executing");
    let started = std::time::Instant::now();
    let execution = record_run_outcome(task, &task_id, ctx);
    let elapsed = started.elapsed();
    ctx.log().record_task_duration(&task_id, elapsed);
    ctx.log().run_task_event(
        LogEvent::TaskTiming,
        &task.log_key(),
        &format!("elapsed {}", format_elapsed(elapsed)),
    );
    execution
}

/// Build a task result with the task's identity and presentation metadata.
pub(crate) fn task_entry(
    task: &dyn Task,
    task_id: &str,
    status: TaskStatus,
    message: Option<&str>,
    actions: ActionCounts,
) -> TaskEntry {
    TaskEntry::new(
        task_id,
        task.name(),
        status,
        message,
        actions,
        task.visibility(),
    )
    .with_selector(task.selector())
    .with_result_display(task.result_display())
}

/// Record a task outcome with the task's own visibility, returning the status.
fn record(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    status: TaskStatus,
    message: Option<&str>,
    actions: ActionCounts,
    message_is_summary: bool,
) -> TaskStatus {
    ctx.log().record_task(
        task_entry(task, task_id, status, message, actions)
            .with_summary_message(message_is_summary),
    );
    status
}

/// Downgrade a cancellation-induced failure to [`TaskStatus::Interrupted`].
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
        .run_task_event(LogEvent::TaskSkip, &task.log_key(), "interrupted");
    ctx.log().warn(format!("interrupted: {detail}"));
    record(
        task,
        task_id,
        ctx,
        TaskStatus::Interrupted,
        Some("interrupted"),
        actions,
        false,
    )
}

/// Presentation and dependency decisions computed before recording a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassifiedResult {
    execution: TaskExecution,
    actions: ActionCounts,
}

/// Classify completed task results without logging or inspecting process state.
fn classify_result(result: &TaskResult, dry_run: bool, require_complete: bool) -> ClassifiedResult {
    let (status, outcome) = match result {
        TaskResult::Ok => (TaskStatus::Ok, TaskOutcome::Satisfied),
        TaskResult::DryRun => (TaskStatus::DryRun, TaskOutcome::Satisfied),
        TaskResult::CheckPassed => (TaskStatus::Passed, TaskOutcome::Satisfied),
        TaskResult::NotApplicable(_) => (TaskStatus::NotApplicable, TaskOutcome::Satisfied),
        TaskResult::Skipped { kind, .. } if kind.is_failure() => (
            if require_complete {
                TaskStatus::Failed
            } else {
                TaskStatus::Skipped
            },
            TaskOutcome::Unmet,
        ),
        TaskResult::Skipped { .. } => (TaskStatus::Skipped, TaskOutcome::Satisfied),
        TaskResult::Failed(_) => (TaskStatus::Failed, TaskOutcome::Failed),
        TaskResult::Batch(stats) => {
            if stats.failed_count() > 0 {
                (TaskStatus::Failed, TaskOutcome::Failed)
            } else {
                let display_status = if stats.changed_count() > 0 {
                    if dry_run {
                        TaskStatus::DryRun
                    } else {
                        TaskStatus::Changed
                    }
                } else if stats.skipped_count() > 0 {
                    TaskStatus::Skipped
                } else {
                    TaskStatus::Ok
                };
                (display_status, TaskOutcome::Satisfied)
            }
        }
    };
    let actions = if let TaskResult::Batch(stats) = result {
        batch_actions(stats, dry_run)
    } else if status == TaskStatus::Failed {
        ActionCounts {
            failed: 1,
            ..ActionCounts::default()
        }
    } else {
        ActionCounts::default()
    };
    ClassifiedResult {
        execution: TaskExecution::new(status, outcome),
        actions,
    }
}

/// Run a task and record its already-classified outcome.
///
/// Typed executor cancellation remains interrupted; unrelated failures retain
/// their failure semantics even when cancellation was requested independently.
fn record_run_outcome(task: &dyn Task, task_id: &str, ctx: &Context) -> TaskExecution {
    ctx.log().task_stage(task.name());
    match task.run(ctx) {
        Ok(result) => {
            let classified = classify_result(&result, ctx.dry_run(), ctx.require_complete());
            let rec = |message: Option<&str>| {
                record(
                    task,
                    task_id,
                    ctx,
                    classified.execution.status,
                    message,
                    classified.actions,
                    false,
                )
            };
            match &result {
                TaskResult::Ok => {
                    ctx.log().run_task_event(LogEvent::TaskDone, task_id, "ok");
                    rec(None);
                }
                TaskResult::DryRun => {
                    ctx.log()
                        .run_task_event(LogEvent::TaskDone, task_id, "planned");
                    rec(None);
                }
                TaskResult::CheckPassed => {
                    ctx.log()
                        .run_task_event(LogEvent::TaskDone, task_id, "passed");
                    rec(None);
                }
                TaskResult::NotApplicable(reason) => {
                    ctx.log()
                        .run_task_event(LogEvent::TaskSkip, task_id, reason);
                    rec(Some(reason));
                }
                TaskResult::Skipped { reason, .. } => {
                    if classified.execution.status == TaskStatus::Failed {
                        record_failed_outcome(task, task_id, ctx, reason, classified);
                    } else {
                        ctx.log()
                            .run_task_event(LogEvent::TaskSkip, task_id, reason);
                        rec(Some(reason));
                    }
                }
                TaskResult::Failed(reason) => {
                    record_failed_outcome(task, task_id, ctx, reason, classified);
                }
                TaskResult::Batch(stats) => {
                    record_batch_outcome(task, task_id, ctx, stats, classified);
                }
            }
            classified.execution
        }
        Err(e) => {
            if let Some(report) = e.downcast_ref::<BatchReport>() {
                return record_stopped_batch(task, task_id, ctx, &e, report);
            }
            if is_interrupted(&e) {
                return TaskExecution::new(
                    record_interrupted(task, task_id, ctx, task.name(), ActionCounts::default()),
                    TaskOutcome::Cancelled,
                );
            }
            let message = format!("{e:#}");
            ctx.log()
                .run_task_event(LogEvent::TaskFail, task_id, &message);
            let summary = concise_failure(&e);
            ctx.log().error(&summary);
            let status = record(
                task,
                task_id,
                ctx,
                TaskStatus::Failed,
                Some(&summary),
                ActionCounts::default(),
                false,
            );
            TaskExecution::new(status, TaskOutcome::Failed)
        }
    }
}

fn record_stopped_batch(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    error: &anyhow::Error,
    report: &BatchReport,
) -> TaskExecution {
    let failed = report.completion() == BatchCompletion::Failed;
    let status = if failed {
        TaskStatus::Failed
    } else {
        TaskStatus::Interrupted
    };
    let mut actions = batch_actions(report.stats(), ctx.dry_run());
    actions.interrupted = report.interrupted_count();
    actions.not_attempted = report.not_attempted_count();
    let summary = report.summary(ctx.dry_run());
    let message = if failed {
        format!("{summary}; {}", concise_failure(error))
    } else {
        format!("interrupted: {summary}")
    };
    ctx.log().run_task_event(
        if failed {
            LogEvent::TaskFail
        } else {
            LogEvent::TaskSkip
        },
        task_id,
        &format!("{message}\n{error:#}"),
    );
    ctx.log().warn(&message);
    TaskExecution::new(
        record(task, task_id, ctx, status, Some(&message), actions, false),
        if failed {
            TaskOutcome::Failed
        } else {
            TaskOutcome::Cancelled
        },
    )
}

/// Keep the task context and first useful child diagnostic on the console.
/// The full chain has already been persisted by the caller.
fn concise_failure(error: &anyhow::Error) -> String {
    let mut context = Vec::new();
    for cause in error.chain() {
        if let Some(exec) = cause.downcast_ref::<ExecError>() {
            return context
                .into_iter()
                .chain([exec.concise_message()])
                .collect::<Vec<_>>()
                .join(": ");
        }
        // Resource errors wrap ExecError and repeat its Display; stop before
        // collecting that wrapper and use its source instead.
        if cause
            .downcast_ref::<crate::engine::resource::ResourceError>()
            .is_none()
        {
            context.push(cause.to_string());
        }
    }
    format!("{error:#}")
}

fn record_failed_outcome(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    reason: &str,
    classified: ClassifiedResult,
) {
    ctx.log()
        .run_task_event(LogEvent::TaskFail, task_id, reason);
    ctx.log().warn(format!("failed: {reason}"));
    record(
        task,
        task_id,
        ctx,
        classified.execution.status,
        Some(reason),
        classified.actions,
        false,
    );
}

fn record_batch_outcome(
    task: &dyn Task,
    task_id: &str,
    ctx: &Context,
    stats: &TaskStats,
    classified: ClassifiedResult,
) {
    let message = stats
        .message()
        .map_or_else(|| stats.summary(ctx.dry_run()), str::to_string);
    let display_status = classified.execution.status;
    let event = if display_status == TaskStatus::Failed {
        LogEvent::TaskFail
    } else {
        LogEvent::TaskDone
    };
    ctx.log().run_task_event(event, task_id, &message);
    if display_status == TaskStatus::Failed {
        ctx.log().warn(format!("failed: {message}"));
    } else if stats.message().is_none() {
        ctx.log().summary(&message);
    } else {
        ctx.log().info(&message);
    }
    let recorded_message = batch_reason(stats, display_status, &message);
    record(
        task,
        task_id,
        ctx,
        display_status,
        recorded_message.as_deref(),
        classified.actions,
        stats.message().is_none()
            && matches!(display_status, TaskStatus::Changed | TaskStatus::Failed),
    );
}

fn batch_actions(stats: &TaskStats, dry_run: bool) -> ActionCounts {
    ActionCounts {
        applied: if dry_run { 0 } else { stats.changed_count() },
        planned: if dry_run { stats.changed_count() } else { 0 },
        skipped: stats.skipped_count(),
        failed: stats.failed_count(),
        ..ActionCounts::default()
    }
}

/// The reason shown on a batch task's status row.
///
/// A skipped batch always states why it did nothing: the aggregate counters are
/// the only reason available once per-item detail has been filtered out, and a
/// bare `SKIPPED` row is the outcome users most often have to ask about.
fn batch_reason(stats: &TaskStats, outcome: TaskStatus, message: &str) -> Option<String> {
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
        TaskStatus::Ok
        | TaskStatus::Passed
        | TaskStatus::DryRun
        | TaskStatus::NotApplicable
        | TaskStatus::Blocked
        | TaskStatus::Interrupted => None,
    }
}

#[cfg(test)]
mod classification_tests {
    use super::*;

    #[test]
    fn classification_preserves_completion_policy() {
        let cases = [
            (
                "ok",
                TaskResult::Ok,
                false,
                false,
                TaskStatus::Ok,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "unquantified preview",
                TaskResult::DryRun,
                false,
                false,
                TaskStatus::DryRun,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "check",
                TaskResult::CheckPassed,
                false,
                true,
                TaskStatus::Passed,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "not applicable",
                TaskResult::NotApplicable("empty".into()),
                false,
                true,
                TaskStatus::NotApplicable,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "benign strict skip",
                TaskResult::skipped("optional"),
                false,
                true,
                TaskStatus::Skipped,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "unmet skip",
                TaskResult::unmet("unavailable"),
                false,
                false,
                TaskStatus::Skipped,
                TaskOutcome::Unmet,
                (0, 0, 0, 0),
            ),
            (
                "strict unmet preview",
                TaskResult::unmet("unavailable"),
                true,
                true,
                TaskStatus::Failed,
                TaskOutcome::Unmet,
                (0, 0, 0, 1),
            ),
            (
                "failure",
                TaskResult::Failed("failed".into()),
                true,
                false,
                TaskStatus::Failed,
                TaskOutcome::Failed,
                (0, 0, 0, 1),
            ),
        ];
        assert_cases(cases);
    }

    #[test]
    fn batch_classification_preserves_precedence() {
        let cases = [
            (
                "empty batch",
                TaskStats::new().finish(),
                false,
                true,
                TaskStatus::Ok,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "current batch",
                TaskStats::from_counts(0, 3, 0, 0).finish(),
                true,
                true,
                TaskStatus::Ok,
                TaskOutcome::Satisfied,
                (0, 0, 0, 0),
            ),
            (
                "skipped batch",
                TaskStats::from_counts(0, 0, 2, 0).finish(),
                false,
                true,
                TaskStatus::Skipped,
                TaskOutcome::Satisfied,
                (0, 0, 2, 0),
            ),
            (
                "changes precede skips",
                TaskStats::from_counts(3, 1, 2, 0).finish(),
                false,
                true,
                TaskStatus::Changed,
                TaskOutcome::Satisfied,
                (3, 0, 2, 0),
            ),
            (
                "planned changes",
                TaskStats::from_counts(3, 1, 2, 0).finish(),
                true,
                true,
                TaskStatus::DryRun,
                TaskOutcome::Satisfied,
                (0, 3, 2, 0),
            ),
            (
                "failure precedes changes",
                TaskStats::from_counts(3, 1, 2, 1).finish(),
                false,
                false,
                TaskStatus::Failed,
                TaskOutcome::Failed,
                (3, 0, 2, 1),
            ),
            (
                "failure precedes preview",
                TaskStats::from_counts(3, 1, 2, 1).finish(),
                true,
                true,
                TaskStatus::Failed,
                TaskOutcome::Failed,
                (0, 3, 2, 1),
            ),
        ];
        assert_cases(cases);
    }

    type Case = (
        &'static str,
        TaskResult,
        bool,
        bool,
        TaskStatus,
        TaskOutcome,
        (u32, u32, u32, u32),
    );

    fn assert_cases<const N: usize>(cases: [Case; N]) {
        for (case, result, dry_run, strict, display_status, outcome, counters) in cases {
            let classified = classify_result(&result, dry_run, strict);
            assert_eq!(
                classified.execution,
                TaskExecution::new(display_status, outcome),
                "{case}"
            );
            assert_eq!(
                classified.actions,
                ActionCounts {
                    applied: counters.0,
                    planned: counters.1,
                    skipped: counters.2,
                    failed: counters.3,
                    ..ActionCounts::default()
                },
                "{case}"
            );
        }
    }
}
