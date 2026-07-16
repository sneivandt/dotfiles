//! Task execution engine: policy evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it evaluates the declarative
//! [`ExecutionPolicy`] rules, decides applicability, runs the task, and records
//! the outcome into the logger.  It deliberately holds no task *definitions* —
//! only the machinery that turns a `&dyn Task` plus a [`Context`] into a
//! recorded result.

use crate::engine::{Context, TaskResult};
use crate::infra::logging::{DiagEvent, TaskStatus, diag_task_context};

use super::{Domain, ExecutionPolicy, Task};

pub(super) fn not_applicable_reason<T: Task + ?Sized>(task: &T, ctx: &Context) -> Option<String> {
    for policy in task.execution_policies() {
        match *policy {
            // Elevation is evaluated in Task::requires_elevation() after
            // platform support and task applicability are known; at execution
            // time this policy is only a declaration.
            ExecutionPolicy::Always | ExecutionPolicy::RequiresElevation => {}
            ExecutionPolicy::PlatformSupported(capability, is_supported) => {
                if !is_supported(&ctx.platform()) {
                    return Some(format!("{capability} not supported on {}", ctx.platform()));
                }
            }
        }
    }
    if task.should_run(ctx) {
        None
    } else {
        Some("not applicable".to_string())
    }
}

fn record_not_applicable(ctx: &Context, name: &str, reason: &str) {
    ctx.log().diag_task(DiagEvent::TaskSkip, name, reason);
    ctx.log().debug_stage(name);
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
    let domain = task.domain();

    if let Some(reason) = not_applicable_reason(task, ctx) {
        record_not_applicable(ctx, task.name(), &reason);
        return TaskStatus::NotApplicable;
    }

    ctx.log()
        .diag_task(DiagEvent::TaskStart, task.name(), "executing");
    record_run_outcome(task, ctx, domain)
}

/// Run a task and record its outcome.
///
/// Cancellation-induced failures (Ctrl-C) are downgraded to
/// [`TaskStatus::Skipped`] so the summary does not count signal
/// interruptions as real failures.
fn record_run_outcome(task: &dyn Task, ctx: &Context, domain: Domain) -> TaskStatus {
    let rec = |status: TaskStatus, msg: Option<&str>| {
        ctx.log().record_task(task.name(), status, msg);
        status
    };
    match task.run_configured(ctx) {
        Ok(None) => {
            ctx.log()
                .diag_task(DiagEvent::TaskSkip, task.name(), "nothing configured");
            ctx.log().debug_stage(task.name());
            ctx.log().debug("nothing configured");
            ctx.log()
                .record_task(task.name(), TaskStatus::NotApplicable, None);
            TaskStatus::NotApplicable
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log().diag_task(DiagEvent::TaskDone, task.name(), "ok");
                let status = if domain == Domain::Validation {
                    TaskStatus::Changed
                } else {
                    TaskStatus::Ok
                };
                rec(status, None)
            }
            TaskResult::OkWithMessage(message) => {
                ctx.log()
                    .diag_task(DiagEvent::TaskDone, task.name(), &message);
                rec(TaskStatus::Changed, Some(&message))
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
                ctx.log().info(&format!("skipped: {reason}"));
                rec(TaskStatus::Skipped, Some(&reason))
            }
            TaskResult::Failed(reason) => {
                if ctx.is_cancelled() {
                    ctx.log()
                        .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                    ctx.log().warn(&format!("interrupted: {reason}"));
                    rec(TaskStatus::Skipped, Some("interrupted"))
                } else {
                    ctx.log()
                        .diag_task(DiagEvent::TaskFail, task.name(), &reason);
                    ctx.log().warn(&format!("failed: {reason}"));
                    rec(TaskStatus::Failed, Some(&reason))
                }
            }
            TaskResult::DryRun => {
                ctx.log()
                    .diag_task(DiagEvent::TaskDone, task.name(), "dry-run");
                rec(TaskStatus::DryRun, None)
            }
        },
        Err(e) => {
            if ctx.is_cancelled() {
                ctx.log()
                    .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                ctx.log().warn(&format!("interrupted: {}", task.name()));
                rec(TaskStatus::Skipped, Some("interrupted"))
            } else {
                ctx.log()
                    .diag_task(DiagEvent::TaskFail, task.name(), &format!("{e:#}"));
                ctx.log().error(&format!("{}: {e:#}", task.name()));
                rec(TaskStatus::Failed, Some(&format!("{e:#}")))
            }
        }
    }
}
