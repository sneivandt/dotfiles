//! Task execution engine: policy evaluation and outcome recording.
//!
//! This module is the runner that the orchestration layer drives.  Given a
//! [`Task`](super::Task) trait object it evaluates the declarative
//! [`ExecutionPolicy`] rules, decides applicability, runs the task, and records
//! the outcome into the logger.  It deliberately holds no task *definitions* —
//! only the machinery that turns a `&dyn Task` plus a [`Context`] into a
//! recorded result.

use crate::engine::{Context, TaskResult};
use crate::logging::{DiagEvent, TaskStatus, diag_task_context};

use super::{Domain, ExecutionPolicy, Task};

#[derive(Debug)]
pub(super) enum PolicyDecision {
    NotApplicable(String),
}

pub(super) fn evaluate_policy_decision(
    policies: &[ExecutionPolicy],
    ctx: &Context,
) -> Option<PolicyDecision> {
    for policy in policies {
        match *policy {
            // Elevation is evaluated in Task::requires_elevation() after
            // platform support and task applicability are known; at execution
            // time this policy is only a declaration.
            ExecutionPolicy::Always | ExecutionPolicy::RequiresElevation => {}
            ExecutionPolicy::PlatformSupported(capability, is_supported) => {
                if !is_supported(&ctx.platform) {
                    return Some(PolicyDecision::NotApplicable(format!(
                        "{capability} not supported on {}",
                        ctx.platform
                    )));
                }
            }
        }
    }
    None
}

fn evaluate_policy(task: &dyn Task, ctx: &Context) -> Option<PolicyDecision> {
    evaluate_policy_decision(task.execution_policies(), ctx)
}

fn record_policy_decision(ctx: &Context, name: &str, domain: Domain, decision: PolicyDecision) {
    match decision {
        PolicyDecision::NotApplicable(reason) => {
            ctx.log.diag_task(DiagEvent::TaskSkip, name, &reason);
            ctx.log.debug_stage(name);
            ctx.debug_fmt(|| format!("not applicable: {reason}"));
            ctx.log
                .record_task_outcome(name, domain, TaskStatus::NotApplicable, None);
        }
    }
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
pub fn execute(task: &dyn Task, ctx: &Context) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    let _diag_context = diag_task_context(task.name());
    let domain = task.domain();

    if let Some(decision) = evaluate_policy(task, ctx) {
        record_policy_decision(ctx, task.name(), domain, decision);
        return;
    }

    if !task.should_run(ctx) {
        ctx.log
            .diag_task(DiagEvent::TaskSkip, task.name(), "not applicable");
        ctx.log.debug_stage(task.name());
        ctx.log
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task_outcome(task.name(), domain, TaskStatus::NotApplicable, None);
        return;
    }

    ctx.log
        .diag_task(DiagEvent::TaskStart, task.name(), "executing");
    record_run_outcome(task, ctx, domain);
}

/// Run a task and record its outcome.
///
/// Cancellation-induced failures (Ctrl-C) are downgraded to
/// [`TaskStatus::Skipped`] so the summary does not count signal
/// interruptions as real failures.
fn record_run_outcome(task: &dyn Task, ctx: &Context, domain: Domain) {
    let rec = |status: TaskStatus, msg: Option<&str>| {
        ctx.log
            .record_task_outcome(task.name(), domain, status, msg);
    };
    match task.run_if_applicable(ctx) {
        Ok(None) => {
            ctx.log
                .diag_task(DiagEvent::TaskSkip, task.name(), "nothing configured");
            ctx.log.debug_stage(task.name());
            ctx.log.debug("nothing configured");
            ctx.log
                .record_task_outcome(task.name(), domain, TaskStatus::NotApplicable, None);
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log.diag_task(DiagEvent::TaskDone, task.name(), "");
                rec(TaskStatus::Ok, None);
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log.diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.debug_fmt(|| format!("not applicable: {reason}"));
                ctx.log
                    .record_task_outcome(task.name(), domain, TaskStatus::NotApplicable, None);
            }
            TaskResult::Skipped(reason) => {
                ctx.log.diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.log.info(&format!("skipped: {reason}"));
                rec(TaskStatus::Skipped, Some(&reason));
            }
            TaskResult::Failed(reason) => {
                if ctx.is_cancelled() {
                    ctx.log
                        .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                    ctx.log.warn(&format!("interrupted: {reason}"));
                    rec(TaskStatus::Skipped, Some("interrupted"));
                } else {
                    ctx.log.diag_task(DiagEvent::TaskFail, task.name(), &reason);
                    ctx.log.warn(&format!("failed: {reason}"));
                    rec(TaskStatus::Failed, Some(&reason));
                }
            }
            TaskResult::DryRun => {
                ctx.log
                    .diag_task(DiagEvent::TaskDone, task.name(), "dry-run");
                rec(TaskStatus::DryRun, None);
            }
        },
        Err(e) => {
            if ctx.is_cancelled() {
                ctx.log
                    .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                ctx.log.warn(&format!("interrupted: {}", task.name()));
                rec(TaskStatus::Skipped, Some("interrupted"));
            } else {
                ctx.log
                    .diag_task(DiagEvent::TaskFail, task.name(), &format!("{e:#}"));
                ctx.log.error(&format!("{}: {e:#}", task.name()));
                rec(TaskStatus::Failed, Some(&format!("{e:#}")));
            }
        }
    }
}
