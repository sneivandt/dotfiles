//! Top-level resource orchestration: check state, dispatch to sequential or
//! parallel processing, and collect stats.

use anyhow::Result;

use super::apply;
use super::context::Context;
use super::mode::ProcessOpts;
use super::parallel;
use super::stats::{TaskResult, TaskStats};
use crate::resources::{Applicable, Resource, ResourceState};

/// Run `process_one` over `items` sequentially, honouring cancellation.
///
/// Centralises the cancellation-aware fold used by every sequential code path.
/// `process_one` returns the per-item [`TaskStats`] delta (or an error that is
/// propagated immediately).
fn run_sequential<T, F>(ctx: &Context, items: Vec<T>, mut process_one: F) -> Result<TaskResult>
where
    F: FnMut(&Context, T) -> Result<TaskStats>,
{
    let mut stats = TaskStats::new();
    for item in items {
        if ctx.is_cancelled() {
            ctx.log.warn("cancelled — stopping before next resource");
            break;
        }
        stats += process_one(ctx, item)?;
    }
    Ok(stats.finish(ctx))
}

/// Process resources by checking each one's current state and applying as needed.
///
/// For tasks where each resource can independently determine its own state via
/// `resource.current_state()`.
///
/// # Errors
///
/// Returns an error if any resource fails to check its state or apply changes,
/// depending on the `bail_on_error` setting in `opts`. If `bail_on_error` is `false`,
/// errors are logged as warnings instead.
pub fn process_resources<R: Resource + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let resources: Vec<R> = resources.into_iter().collect();
    let span = tracing::debug_span!(
        "process_resources",
        verb = opts.verb,
        count = resources.len()
    );
    let _enter = span.enter();
    if ctx.parallel && !opts.sequential && resources.len() > 1 {
        ctx.debug_fmt(|| format!("processing {} resources in parallel", resources.len()));
        return parallel::process_resources_parallel(ctx, resources, opts);
    }
    run_sequential(ctx, resources, |ctx, resource| {
        let current = resource.current_state()?;
        apply::process_single(ctx, &resource, &current, opts)
    })
}

/// Process resources with pre-computed states.
///
/// For tasks that batch-query state (e.g., registry, packages, VS Code extensions)
/// and then iterate with cached results.
///
/// # Errors
///
/// Returns an error if any resource fails to apply changes, depending on the
/// `bail_on_error` setting in `opts`. If `bail_on_error` is `false`, errors are
/// logged as warnings instead.
pub fn process_resource_states<R: Applicable + Send>(
    ctx: &Context,
    resource_states: impl IntoIterator<Item = (R, ResourceState)>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let resource_states: Vec<(R, ResourceState)> = resource_states.into_iter().collect();
    let span = tracing::debug_span!(
        "process_resource_states",
        verb = opts.verb,
        count = resource_states.len()
    );
    let _enter = span.enter();
    if ctx.parallel && !opts.sequential && resource_states.len() > 1 {
        ctx.debug_fmt(|| format!("processing {} resources in parallel", resource_states.len()));
        return parallel::process_resource_states_parallel(ctx, resource_states, opts);
    }
    run_sequential(ctx, resource_states, |ctx, (resource, current)| {
        apply::process_single(ctx, &resource, &current, opts)
    })
}

/// Process resources for removal.
///
/// Only resources in [`ResourceState::Correct`] are removed (they are "ours").
/// Resources that are `Missing`, `Incorrect`, or `Invalid` are skipped.
///
/// When `ctx.parallel` is `true` and there is more than one resource, removal
/// runs in parallel using Rayon (matching the behaviour of [`process_resources`]
/// and [`process_resource_states`]).
///
/// # Errors
///
/// Returns an error if a resource fails to check its current state or fails
/// during the removal process.
pub fn process_resources_remove<R: Resource + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    verb: &str,
) -> Result<TaskResult> {
    let resources: Vec<R> = resources.into_iter().collect();
    let span = tracing::debug_span!("process_resources_remove", verb, count = resources.len());
    let _enter = span.enter();
    if ctx.parallel && resources.len() > 1 {
        ctx.debug_fmt(|| format!("processing {} resources in parallel", resources.len()));
        return parallel::process_remove_parallel(ctx, resources, verb);
    }
    run_sequential(ctx, resources, |ctx, resource| {
        let current = match resource.current_state() {
            Ok(current) => current,
            Err(e) => {
                ctx.log.warn(&format!(
                    "failed to check state for {}: {e}",
                    resource.description()
                ));
                return Ok(TaskStats::new());
            }
        };
        apply::remove_single(ctx, &resource, &current, verb)
    })
}
