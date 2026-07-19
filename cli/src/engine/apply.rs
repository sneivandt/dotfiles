//! Single-resource processing: check state, apply or remove one resource.

use anyhow::Result;

use super::context::Context;
use super::mode::ProcessOpts;
use super::plan::{ApplyChange, ApplyOperation, RemoveChange, RemoveOperation};
use super::stats::TaskStats;
use crate::engine::{Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::logging::DiagEvent;

/// Process a single resource given its current state, returning a stats delta.
pub(super) fn process_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    resource_state: &ResourceState,
    opts: &ProcessOpts,
) -> Result<TaskStats> {
    let plan = ApplyChange::from_state(resource.description(), resource_state, opts);
    ctx.log().diag(
        DiagEvent::ResourceCheck,
        &format!("{} state={resource_state}", plan.description()),
    );
    let mut delta = TaskStats::new();
    match plan.operation() {
        ApplyOperation::Noop => {
            ctx.debug_fmt(|| format!("ok: {}", plan.description()));
            delta.already_ok = delta.already_ok.saturating_add(1);
        }
        ApplyOperation::Skip { reason, failed } => {
            ctx.debug_fmt(|| format!("skipping {}: {reason}", plan.description()));
            if *failed {
                delta.failed = delta.failed.saturating_add(1);
            } else {
                delta.skipped = delta.skipped.saturating_add(1);
            }
        }
        ApplyOperation::Apply {
            verb,
            bail_on_error,
            ..
        } => {
            delta.merge(&execute_mutation(
                ctx,
                resource,
                ResourceMutation {
                    description: plan.description(),
                    verb,
                    dry_run_message: plan.dry_run_message(),
                    event: DiagEvent::ResourceApply,
                    applied_label: "applied",
                    bail_on_error: *bail_on_error,
                    warn_before_apply: true,
                },
                Resource::apply,
            )?);
        }
    }
    Ok(delta)
}

/// Record the outcome of a single resource change, updating `delta` and
/// emitting the appropriate log events.
///
/// `verb` is the human-facing action word used in dry-run and error output
/// (e.g. `"link"` or `"unlink"`). `applied_label` is the past-tense word used
/// in the diagnostic trace (`"applied"` or `"removed"`).
fn record_resource_change(
    ctx: &Context,
    delta: &mut TaskStats,
    change: ResourceChange,
    desc: &str,
    verb: &str,
    applied_label: &str,
) {
    match change {
        ResourceChange::Applied => {
            ctx.log().diag(
                DiagEvent::ResourceResult,
                &format!("{desc} {applied_label}"),
            );
            ctx.log().info(&format!("{verb} {desc}"));
            delta.changed = delta.changed.saturating_add(1);
        }
        ResourceChange::AlreadyCorrect => {
            ctx.log().diag(
                DiagEvent::ResourceResult,
                &format!("{desc} already_correct"),
            );
            delta.already_ok = delta.already_ok.saturating_add(1);
        }
        ResourceChange::Skipped { reason } => {
            ctx.log().diag(
                DiagEvent::ResourceResult,
                &format!("{desc} skipped: {reason}"),
            );
            ctx.log().warn(&format!("skipping {desc}: {reason}"));
            delta.failed = delta.failed.saturating_add(1);
        }
    }
}

#[derive(Debug)]
struct ResourceMutation<'a> {
    description: &'a str,
    verb: &'a str,
    dry_run_message: Option<String>,
    event: DiagEvent,
    applied_label: &'a str,
    bail_on_error: bool,
    warn_before_apply: bool,
}

fn execute_mutation<R, F>(
    ctx: &Context,
    resource: &R,
    mutation: ResourceMutation<'_>,
    mutate: F,
) -> Result<TaskStats>
where
    R: Resource,
    F: FnOnce(&R) -> ResourceResult<ResourceChange>,
{
    if ctx.dry_run() {
        if let Some(message) = mutation.dry_run_message {
            ctx.log().dry_run(&message);
        }
        let mut delta = TaskStats::new();
        delta.changed = delta.changed.saturating_add(1);
        return Ok(delta);
    }
    if mutation.warn_before_apply
        && let Some(warning) = resource.pre_apply_warning()?
    {
        ctx.log().warn(&warning);
    }
    ctx.log().diag(
        mutation.event,
        &format!("{} {}", mutation.verb, mutation.description),
    );
    let mut delta = TaskStats::new();
    let change = match mutate(resource) {
        Ok(change) => change,
        Err(e) => {
            let category = e.category();
            ctx.log().diag(
                DiagEvent::ResourceResult,
                &format!("{} error [{category}]: {e}", mutation.description),
            );
            if mutation.bail_on_error {
                return Err(e.into());
            }
            ctx.log().warn(&format!(
                "failed to {} {}: {e}",
                mutation.verb, mutation.description
            ));
            delta.failed = delta.failed.saturating_add(1);
            return Ok(delta);
        }
    };

    record_resource_change(
        ctx,
        &mut delta,
        change,
        mutation.description,
        mutation.verb,
        mutation.applied_label,
    );
    Ok(delta)
}

/// Remove a single resource, returning a stats delta.
pub(super) fn remove_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    current: &ResourceState,
    verb: &str,
) -> Result<TaskStats> {
    let plan = RemoveChange::from_state(resource.description(), current, verb);
    let mut delta = TaskStats::new();
    match plan.operation() {
        RemoveOperation::Remove { verb: remove_verb } => {
            delta.merge(&execute_mutation(
                ctx,
                resource,
                ResourceMutation {
                    description: plan.description(),
                    verb: remove_verb,
                    dry_run_message: plan.dry_run_message(),
                    event: DiagEvent::ResourceRemove,
                    applied_label: "removed",
                    bail_on_error: true,
                    warn_before_apply: false,
                },
                Resource::remove,
            )?);
        }
        RemoveOperation::Skip { reason } => {
            // Cannot determine if this resource is ours — skip removal rather
            // than risking removing something we did not install.
            ctx.log().warn(&format!(
                "skipping removal of {}: {reason}",
                plan.description()
            ));
            delta.skipped = delta.skipped.saturating_add(1);
        }
        RemoveOperation::Noop => {
            // Not ours or doesn't exist — skip silently
            delta.already_ok = delta.already_ok.saturating_add(1);
        }
    }
    Ok(delta)
}
