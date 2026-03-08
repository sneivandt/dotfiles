//! Single-resource processing: check state, apply or remove one resource.

use anyhow::Result;

use super::context::Context;
use super::mode::{ProcessOpts, ResourceAction};
use super::stats::TaskStats;
use crate::error::ResourceError;
use crate::logging::DiagEvent;
use crate::resources::{Applicable, ResourceChange, ResourceState};

/// Process a single resource given its current state, returning a stats delta.
pub(super) fn process_single<R: Applicable>(
    ctx: &Context,
    resource: &R,
    resource_state: &ResourceState,
    opts: &ProcessOpts,
) -> Result<TaskStats> {
    let desc = resource.description();
    if let Some(diag) = ctx.log.diagnostic() {
        diag.emit(
            DiagEvent::ResourceCheck,
            &format!("{desc} state={resource_state}"),
        );
    }
    let mut delta = TaskStats::new();
    match opts.mode.action_for(resource_state) {
        ResourceAction::Noop => {
            ctx.debug_fmt(|| format!("ok: {desc}"));
            delta.already_ok += 1;
        }
        ResourceAction::Skip(reason) => {
            ctx.debug_fmt(|| format!("skipping {desc}: {reason}"));
            delta.skipped += 1;
        }
        ResourceAction::Apply => {
            if ctx.dry_run {
                let msg = if let ResourceState::Incorrect { ref current } = *resource_state {
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
fn apply_resource<R: Applicable>(
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
            let category = categorize_error(&e);
            if let Some(diag) = ctx.log.diagnostic() {
                diag.emit(
                    DiagEvent::ResourceResult,
                    &format!("{desc} error [{category}]: {e}"),
                );
            }
            if opts.mode.bail_on_error() {
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
            ctx.debug_fmt(|| format!("{}: {desc}", opts.verb));
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
            ctx.log.warn(&format!("skipping {desc}: {reason}"));
            delta.skipped += 1;
        }
    }
    Ok(delta)
}

/// Remove a single resource, returning a stats delta.
pub(super) fn remove_single<R: Applicable>(
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
            ctx.debug_fmt(|| format!("{verb}: {desc}"));
            delta.changed += 1;
        }
        ResourceState::Unknown { reason } => {
            // Cannot determine if this resource is ours — skip removal rather
            // than risking removing something we did not install.
            ctx.log.warn(&format!(
                "skipping removal of {desc}: state unknown ({reason})"
            ));
            delta.skipped += 1;
        }
        _ => {
            // Not ours or doesn't exist — skip silently
            delta.already_ok += 1;
        }
    }
    Ok(delta)
}

/// Categorise an error for diagnostic logging.
///
/// Attempts to downcast the `anyhow::Error` to a [`ResourceError`] for a
/// structured category label. Falls back to `"unknown"` for untyped errors.
fn categorize_error(e: &anyhow::Error) -> &'static str {
    match e.downcast_ref::<ResourceError>() {
        Some(ResourceError::CommandFailed { .. }) => "command_failed",
        Some(ResourceError::PermissionDenied { .. }) => "permission_denied",
        Some(ResourceError::ConflictingState { .. }) => "conflicting_state",
        Some(ResourceError::NotSupported { .. }) => "not_supported",
        None => "unknown",
    }
}
