//! Top-level resource orchestration: check state, dispatch to sequential or
//! parallel processing, and collect stats.

use anyhow::{Context as _, Result};

use super::apply;
use super::batch::BatchProgress;
use super::context::Context;
use super::mode::ProcessOpts;
use super::parallel;
use super::stats::{TaskResult, TaskStats};
use crate::engine::{
    AccessClaim, IntrinsicState, RemovableResource, Resource, ResourceResult, ResourceState,
};
use crate::infra::logging::OutputExt as _;

/// Dispatch item processing, retaining completed outcomes when dispatch stops.
fn process_items<T: Send>(
    ctx: &Context,
    items: Vec<T>,
    sequential: bool,
    access_claims: impl Fn(&T) -> Vec<AccessClaim>,
    process_one: impl Fn(T) -> Result<TaskStats> + Sync + Send,
) -> Result<TaskResult> {
    let mut pending = Vec::with_capacity(items.len());
    for item in items {
        let claims = access_claims(&item);
        ctx.validate_access(&claims)?;
        pending.push((item, claims));
    }
    let mut progress = BatchProgress::default();
    while !pending.is_empty() {
        if ctx.is_cancelled() {
            ctx.log().warn("cancelled — stopping before next resource");
            progress.omit(pending.len());
            break;
        }
        // Reserve on the caller, never on a Rayon worker: a task holding a
        // conflicting claim may itself need the entire pool to make progress.
        let mut ready = Vec::new();
        let mut waiting = Vec::new();
        for (index, (item, claims)) in pending.into_iter().enumerate() {
            if (!sequential || index == 0)
                && let Some(guard) = ctx.try_access(&claims)
            {
                ready.push((item, guard));
            } else {
                waiting.push((item, claims));
            }
        }
        pending = waiting;
        if ready.is_empty() {
            if ctx.wait_for_access().is_err() {
                ctx.log().warn("cancelled — stopping before next resource");
                progress.omit(pending.len());
                break;
            }
            continue;
        }
        let wave = if ctx.parallel() && !sequential && ready.len() > 1 {
            parallel::collect_parallel_stats(ctx, ready, |(item, _guard)| process_one(item))
        } else {
            let mut wave = BatchProgress::default();
            let mut ready = ready.into_iter();
            while let Some((item, _guard)) = ready.next() {
                if ctx.is_cancelled() {
                    wave.omit(ready.len().saturating_add(1));
                    break;
                }
                if wave.record(process_one(item)) {
                    wave.omit(ready.len());
                    break;
                }
            }
            wave
        };
        let stopped = wave.stopped();
        progress = progress.merge(wave);
        if stopped {
            progress.omit(pending.len());
            break;
        }
    }
    progress.finish()
}

/// Process resources with a state-discovery function.
///
/// The function may check each resource intrinsically or answer from bulk state
/// the caller gathered before the batch started.
///
/// # Errors
///
/// Returns an error if per-resource state checking or applying changes fails,
/// depending on the `bail_on_error` setting in `opts`.
pub(super) fn process_resources_with_state<R>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    state: impl Fn(&R) -> ResourceResult<ResourceState> + Sync,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: Resource + Send,
{
    let resources: Vec<R> = resources.into_iter().collect();
    if resources.is_empty() {
        return Ok(TaskResult::Ok);
    }

    let span = tracing::debug_span!(
        "process_apply_items",
        kind = "state_discovery",
        verb = opts.verb,
        count = resources.len()
    );
    let _enter = span.enter();
    process_items(
        ctx,
        resources,
        opts.sequential || !ctx.parallel(),
        Resource::access_claims,
        |resource| {
            let current = state(&resource)
                .with_context(|| format!("failed to check {}", resource.description()))?;
            apply::process_single(ctx, &resource, &current, opts)
        },
    )
}

/// Process resources whose current state is derived from a borrowed cache.
///
/// Do not snapshot mutable claimed state before entering this function. Gather
/// that state inside a task-level claim, or use intrinsic discovery instead.
///
/// # Errors
///
/// Returns an error if resource state discovery or processing fails.
pub fn process_resources_with_cache<R, Cache, State>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    cache: &Cache,
    state: State,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: Resource + Send,
    Cache: Sync + ?Sized,
    State: for<'a> Fn(&'a R, &Cache) -> ResourceResult<ResourceState> + Sync,
{
    process_resources_with_state(ctx, resources, |resource| state(resource, cache), opts)
}

/// Process resources by checking each one's intrinsic current state.
///
/// This is a convenience wrapper around [`process_resources_with_state`] for
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
    process_resources_with_state(ctx, resources, IntrinsicState::current_state, opts)
}

/// Process resources for removal.
///
/// Only resources in [`ResourceState::Correct`] are removed (they are "ours").
/// Resources that are `Missing`, `Incorrect`, or `Invalid` are skipped.
/// `Unknown` resources are not removed and count as failures because ownership
/// could not be verified.
///
/// When `ctx.parallel` is `true` and there is more than one resource, removal
/// runs in parallel using Rayon (matching the behaviour of [`process_resources`]
/// and [`process_resources_with_state`]).
///
/// # Errors
///
/// Returns an error if a resource fails to check its current state or fails
/// during the removal process.
pub fn process_resources_remove<R: IntrinsicState + RemovableResource + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    verb: &'static str,
) -> Result<TaskResult> {
    let resources: Vec<R> = resources.into_iter().collect();
    let span = tracing::debug_span!("process_resources_remove", verb, count = resources.len());
    let _enter = span.enter();
    process_items(
        ctx,
        resources,
        !ctx.parallel(),
        Resource::access_claims,
        |resource| {
            let current = resource.current_state().with_context(|| {
                format!("failed to check {} for removal", resource.description())
            })?;
            apply::remove_single(ctx, &resource, &current, verb)
        },
    )
}
