//! Single-resource processing: check state, apply or remove one resource.

use anyhow::Result;

use super::context::Context;
use super::{ProcessOpts, TaskStats};
use crate::logging::DiagEvent;
use crate::resources::{Resource, ResourceChange, ResourceState};

/// Process a single resource given its current state, returning a stats delta.
pub(super) fn process_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    resource_state: ResourceState,
    opts: &ProcessOpts,
) -> Result<TaskStats> {
    let desc = resource.description();
    if let Some(diag) = ctx.log.diagnostic() {
        diag.emit(
            DiagEvent::ResourceCheck,
            &format!("{desc} state={resource_state:?}"),
        );
    }
    let mut delta = TaskStats::new();
    match resource_state {
        ResourceState::Correct => {
            ctx.log.debug(&format!("ok: {desc}"));
            delta.already_ok += 1;
        }
        ResourceState::Invalid { reason } => {
            ctx.log.debug(&format!("skipping {desc}: {reason}"));
            delta.skipped += 1;
        }
        ResourceState::Missing if !opts.fix_missing => {
            delta.skipped += 1;
        }
        ResourceState::Incorrect { .. } if !opts.fix_incorrect => {
            ctx.log
                .debug(&format!("skipping {desc} (unexpected state)"));
            delta.skipped += 1;
        }
        resource_state @ (ResourceState::Missing | ResourceState::Incorrect { .. }) => {
            if ctx.dry_run {
                let msg = if let ResourceState::Incorrect { ref current } = resource_state {
                    format!("would {} {desc} (currently {current})", opts.verb)
                } else {
                    format!("would {}: {desc}", opts.verb)
                };
                ctx.log.dry_run(&msg);
                delta.changed += 1;
                return Ok(delta);
            }
            delta += apply_resource(ctx, resource, opts)?;
        }
    }
    Ok(delta)
}

/// Apply a single resource change, returning a stats delta.
pub(super) fn apply_resource<R: Resource>(
    ctx: &Context,
    resource: &R,
    opts: &ProcessOpts,
) -> Result<TaskStats> {
    let desc = resource.description();
    if let Some(diag) = ctx.log.diagnostic() {
        diag.emit(DiagEvent::ResourceApply, &format!("{} {desc}", opts.verb));
    }
    let mut delta = TaskStats::new();
    let change = match resource.apply() {
        Ok(change) => change,
        Err(e) => {
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(DiagEvent::ResourceResult, &format!("{desc} error: {e}"));
            }
            if opts.bail_on_error {
                return Err(e);
            }
            ctx.log
                .warn(&format!("failed to {} {desc}: {e}", opts.verb));
            delta.skipped += 1;
            return Ok(delta);
        }
    };

    match change {
        ResourceChange::Applied => {
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(DiagEvent::ResourceResult, &format!("{desc} applied"));
            }
            ctx.log.debug(&format!("{}: {desc}", opts.verb));
            delta.changed += 1;
        }
        ResourceChange::AlreadyCorrect => {
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(
                    DiagEvent::ResourceResult,
                    &format!("{desc} already_correct"),
                );
            }
            delta.already_ok += 1;
        }
        ResourceChange::Skipped { reason } => {
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(
                    DiagEvent::ResourceResult,
                    &format!("{desc} skipped: {reason}"),
                );
            }
            if opts.bail_on_error {
                anyhow::bail!("failed to {} {desc}: {reason}", opts.verb);
            }
            ctx.log
                .warn(&format!("failed to {} {desc}: {reason}", opts.verb));
            delta.skipped += 1;
        }
    }
    Ok(delta)
}

/// Remove a single resource, returning a stats delta.
pub(super) fn remove_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    current: &ResourceState,
    verb: &str,
) -> Result<TaskStats> {
    let desc = resource.description();
    let mut delta = TaskStats::new();
    match current {
        ResourceState::Correct => {
            if ctx.dry_run {
                ctx.log.dry_run(&format!("would {verb}: {desc}"));
                delta.changed += 1;
                return Ok(delta);
            }
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(DiagEvent::ResourceRemove, &format!("{verb} {desc}"));
            }
            resource.remove()?;
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(DiagEvent::ResourceResult, &format!("{desc} removed"));
            }
            ctx.log.debug(&format!("{verb}: {desc}"));
            delta.changed += 1;
        }
        _ => {
            // Not ours or doesn't exist — skip silently
            delta.already_ok += 1;
        }
    }
    Ok(delta)
}
