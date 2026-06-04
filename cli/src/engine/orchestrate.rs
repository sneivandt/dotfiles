//! Top-level resource orchestration: check state, dispatch to sequential or
//! parallel processing, and collect stats.

use anyhow::Result;

use super::apply;
use super::context::Context;
use super::mode::ProcessOpts;
use super::parallel;
use super::stats::{TaskResult, TaskStats};
use crate::resources::{
    IntrinsicState, IntrinsicStateProvider, Resource, ResourceState, ResourceStateProvider,
};

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
        #[allow(
            clippy::arithmetic_side_effects,
            reason = "TaskStats::add_assign saturates internally"
        )]
        {
            stats += process_one(ctx, item)?;
        }
    }
    Ok(stats.finish(ctx))
}

fn process_apply_items<T, R>(
    ctx: &Context,
    items: Vec<T>,
    opts: &ProcessOpts,
    span_kind: &'static str,
    get_resource_state: impl Fn(T) -> Result<(R, ResourceState)> + Sync + Send,
) -> Result<TaskResult>
where
    T: Send,
    R: Resource + Send,
{
    let span = tracing::debug_span!(
        "process_apply_items",
        kind = span_kind,
        verb = opts.verb,
        count = items.len()
    );
    let _enter = span.enter();
    if ctx.parallel && !opts.sequential && items.len() > 1 {
        ctx.debug_fmt(|| format!("processing {} resources in parallel", items.len()));
        return parallel::process_apply_parallel(ctx, items, opts, get_resource_state);
    }
    run_sequential(ctx, items, |ctx, item| {
        let (resource, current) = get_resource_state(item)?;
        apply::process_single(ctx, &resource, &current, opts)
    })
}

/// Process resources with an explicit state provider.
///
/// The provider may check each resource intrinsically or load cached/bulk state
/// once for the full resource slice.
///
/// # Errors
///
/// Returns an error if provider state loading, per-resource state checking, or
/// applying changes fails, depending on the `bail_on_error` setting in `opts`.
pub fn process_resources_with_provider<R, P>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    provider: &P,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: Resource + Send,
    P: ResourceStateProvider<R> + Sync,
    P::Cache: Sync,
{
    let resources: Vec<R> = resources.into_iter().collect();
    if resources.is_empty() {
        return Ok(TaskResult::Ok);
    }

    let cache = provider.load(&resources)?;
    process_apply_items(ctx, resources, opts, "state_provider", |resource| {
        let state = provider.current_state(&resource, &cache)?;
        Ok((resource, state))
    })
}

/// Process resources by checking each one's intrinsic current state.
///
/// This is a convenience wrapper around [`process_resources_with_provider`] for
/// resources that implement [`IntrinsicState`].
///
/// # Errors
///
/// Returns an error if any resource fails to check its state or apply changes,
/// depending on the `bail_on_error` setting in `opts`.
pub fn process_resources<R: IntrinsicState + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    process_resources_with_provider(ctx, resources, &IntrinsicStateProvider, opts)
}

/// Process resources for removal.
///
/// Only resources in [`ResourceState::Correct`] are removed (they are "ours").
/// Resources that are `Missing`, `Incorrect`, or `Invalid` are skipped.
///
/// When `ctx.parallel` is `true` and there is more than one resource, removal
/// runs in parallel using Rayon (matching the behaviour of [`process_resources`]
/// and [`process_resources_with_provider`]).
///
/// # Errors
///
/// Returns an error if a resource fails to check its current state or fails
/// during the removal process.
pub fn process_resources_remove<R: IntrinsicState + Send>(
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
        let current = resource.current_state()?;
        apply::remove_single(ctx, &resource, &current, verb)
    })
}
