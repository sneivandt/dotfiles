//! Single-resource processing: check state, apply or remove one resource.

use anyhow::Result;

use super::context::Context;
use super::mode::ProcessOpts;
use super::plan::{ApplyChange, ApplyOperation, RemoveChange, RemoveOperation};
use super::stats::{ItemOutcome, TaskStats};
use crate::engine::{RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::logging::LogEvent;
use crate::infra::logging::OutputExt as _;

/// Process a single resource given its current state, returning a stats delta.
pub(super) fn process_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    resource_state: &ResourceState,
    opts: &ProcessOpts,
) -> Result<TaskStats> {
    let plan = ApplyChange::from_state(resource.description(), resource_state, opts);
    ctx.log().run_event(
        LogEvent::ResourceCheck,
        &format!("{} state={resource_state}", plan.description()),
    );
    let mut delta = TaskStats::new();
    match plan.operation() {
        ApplyOperation::Noop => {
            ctx.debug_fmt(|| format!("ok: {}", plan.description()));
            delta.record(ItemOutcome::AlreadyOk);
        }
        ApplyOperation::Skip { reason, kind } => {
            if kind.is_failure() {
                ctx.log()
                    .warn(format!("skipping {}: {reason}", plan.description()));
            } else {
                ctx.debug_fmt(|| format!("skipping {}: {reason}", plan.description()));
            }
            delta.record(if kind.is_failure() {
                ItemOutcome::Failed
            } else {
                ItemOutcome::Skipped
            });
        }
        ApplyOperation::Apply {
            verb,
            bail_on_error,
            ..
        } => {
            delta.merge(&execute_mutation(
                ctx,
                resource,
                ResourceMutation::apply(
                    plan.description(),
                    verb,
                    plan.dry_run_message(),
                    *bail_on_error,
                ),
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
            ctx.log()
                .run_event(LogEvent::ResourceResult, &format!("{desc} {applied_label}"));
            ctx.log().info(format!("{verb} {desc}"));
            delta.record(ItemOutcome::Changed);
        }
        ResourceChange::AlreadyCorrect => {
            ctx.log()
                .run_event(LogEvent::ResourceResult, &format!("{desc} already_correct"));
            delta.record(ItemOutcome::AlreadyOk);
        }
        ResourceChange::Skipped { reason, kind } => {
            ctx.log().run_event(
                LogEvent::ResourceResult,
                &format!("{desc} skipped: {reason}"),
            );
            ctx.log().warn(format!("skipping {desc}: {reason}"));
            delta.record(if kind.is_failure() {
                ItemOutcome::Failed
            } else {
                ItemOutcome::Skipped
            });
        }
    }
}

#[derive(Debug)]
struct ResourceMutation<'a> {
    description: &'a str,
    verb: &'a str,
    dry_run_message: Option<String>,
    event: LogEvent,
    applied_label: &'a str,
    bail_on_error: bool,
    warn_before_apply: bool,
}

impl<'a> ResourceMutation<'a> {
    const fn apply(
        description: &'a str,
        verb: &'a str,
        dry_run_message: Option<String>,
        bail_on_error: bool,
    ) -> Self {
        Self {
            description,
            verb,
            dry_run_message,
            event: LogEvent::ResourceApply,
            applied_label: "applied",
            bail_on_error,
            warn_before_apply: true,
        }
    }

    const fn remove(description: &'a str, verb: &'a str, dry_run_message: Option<String>) -> Self {
        Self {
            description,
            verb,
            dry_run_message,
            event: LogEvent::ResourceRemove,
            applied_label: "removed",
            bail_on_error: true,
            warn_before_apply: false,
        }
    }
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
        delta.record(ItemOutcome::Changed);
        return Ok(delta);
    }
    if mutation.warn_before_apply
        && let Some(warning) = resource.pre_apply_warning()?
    {
        ctx.log().warn(&warning);
    }
    ctx.log().run_event(
        mutation.event,
        &format!("{} {}", mutation.verb, mutation.description),
    );
    let mut delta = TaskStats::new();
    let change = match mutate(resource) {
        Ok(change) => change,
        Err(e) => {
            if e.is_cancelled() {
                return Err(anyhow::Error::new(e).context(format!(
                    "failed to {} {}",
                    mutation.verb, mutation.description
                )));
            }
            let category = e.category();
            ctx.log().run_event(
                LogEvent::ResourceResult,
                &format!("{} error [{category}]: {e}", mutation.description),
            );
            if mutation.bail_on_error {
                // Keep the failing resource identifiable once the error leaves
                // the engine: the typed error alone says what went wrong, not
                // which resource it went wrong on.
                return Err(anyhow::Error::new(e).context(format!(
                    "failed to {} {}",
                    mutation.verb, mutation.description
                )));
            }
            ctx.log().warn(format!(
                "failed to {} {}: {e}",
                mutation.verb, mutation.description
            ));
            delta.record(ItemOutcome::Failed);
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
pub(super) fn remove_single<R: RemovableResource>(
    ctx: &Context,
    resource: &R,
    current: &ResourceState,
    verb: &'static str,
) -> Result<TaskStats> {
    let plan = RemoveChange::from_state(resource.description(), current, verb);
    let mut delta = TaskStats::new();
    match plan.operation() {
        RemoveOperation::Remove { verb: remove_verb } => {
            delta.merge(&execute_mutation(
                ctx,
                resource,
                ResourceMutation::remove(plan.description(), remove_verb, plan.dry_run_message()),
                RemovableResource::remove,
            )?);
        }
        RemoveOperation::Skip { reason } => {
            // Cannot determine if this resource is ours — skip removal rather
            // than risking removing something we did not install.
            ctx.log().warn(format!(
                "skipping removal of {}: {reason}",
                plan.description()
            ));
            delta.record(ItemOutcome::Skipped);
        }
        RemoveOperation::Noop => {
            // Not ours or doesn't exist — skip silently
            delta.record(ItemOutcome::AlreadyOk);
        }
    }
    Ok(delta)
}
