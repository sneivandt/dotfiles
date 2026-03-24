//! Rayon-based parallel resource processing.

use anyhow::Result;

use super::apply::{process_single, remove_single};
use super::context::Context;
use super::mode::ProcessOpts;
use super::stats::TaskStats;
use crate::logging::{diag_thread_name, set_diag_thread_name};
use crate::resources::{Applicable, Resource, ResourceState};

/// Process resources in parallel using Rayon.
pub(super) fn process_resources_parallel<R: Resource + Send>(
    ctx: &Context,
    resources: Vec<R>,
    opts: &ProcessOpts,
) -> Result<super::stats::TaskResult> {
    run_parallel(ctx, resources, opts, |resource| {
        let state = resource.current_state()?;
        Ok((resource, state))
    })
}

/// Process resources with pre-computed states in parallel using Rayon.
pub(super) fn process_resource_states_parallel<R: Applicable + Send>(
    ctx: &Context,
    resource_states: Vec<(R, ResourceState)>,
    opts: &ProcessOpts,
) -> Result<super::stats::TaskResult> {
    run_parallel(ctx, resource_states, opts, Ok)
}

/// Remove resources in parallel using Rayon.
pub(super) fn process_remove_parallel<R: Resource + Send>(
    ctx: &Context,
    resources: Vec<R>,
    verb: &str,
) -> Result<super::stats::TaskResult> {
    let cancelled = ctx.cancelled.clone();
    let stats = collect_parallel_stats(
        resources,
        |resource| {
            let current = match resource.current_state() {
                Ok(state) => state,
                Err(e) => {
                    // Log the error and skip this resource rather than aborting
                    // the entire batch, matching the apply path's resilient
                    // behaviour.
                    ctx.log.warn(&format!(
                        "failed to check state for {}: {e}",
                        resource.description()
                    ));
                    return Ok(TaskStats::default());
                }
            };
            remove_single(ctx, &resource, &current, verb)
        },
        move || cancelled.is_cancelled(),
    )?;
    Ok(stats.finish(ctx))
}

/// Accumulate per-item [`TaskStats`] deltas in parallel using Rayon.
///
/// Runs `work` on each item concurrently using Rayon's `try_fold` /
/// `try_reduce` pattern: each thread accumulates a local `TaskStats` without
/// any synchronisation, and the per-thread results are merged in a tree
/// reduction at the end.  This avoids the contention of a shared
/// `Mutex<TaskStats>` without changing observable behaviour.
///
/// The diagnostic thread name is captured once before dispatching and re-set
/// on each iteration so the log timeline remains accurate even when Rayon
/// reuses threads across work items (a stale name is harmless but misleading).
///
/// When `cancelled` returns `true`, new items are skipped so that in-flight
/// operations can finish cleanly.
fn collect_parallel_stats<T: Send>(
    items: Vec<T>,
    work: impl Fn(T) -> Result<TaskStats> + Sync + Send,
    cancelled: impl Fn() -> bool + Sync + Send,
) -> Result<TaskStats> {
    use rayon::prelude::*;
    let task_name = diag_thread_name();
    items
        .into_par_iter()
        .try_fold(TaskStats::default, |mut acc, item| {
            set_diag_thread_name(&task_name);
            if cancelled() {
                return Ok(acc);
            }
            acc += work(item)?;
            Ok(acc)
        })
        .try_reduce(TaskStats::default, |mut a, b| {
            a += b;
            Ok(a)
        })
}

/// Generic parallel processing helper using Rayon.
///
/// Accepts a vector of items and a closure that extracts a `(Resource, ResourceState)`
/// pair from each item. The closure runs in parallel and delegates stats
/// accumulation to [`collect_parallel_stats`], which keeps per-thread
/// [`TaskStats`] locally and merges them at the end without a shared lock.
fn run_parallel<T: Send, R: Applicable + Send>(
    ctx: &Context,
    items: Vec<T>,
    opts: &ProcessOpts,
    get_resource_state: impl Fn(T) -> Result<(R, ResourceState)> + Sync,
) -> Result<super::stats::TaskResult> {
    let cancelled = ctx.cancelled.clone();
    let stats = collect_parallel_stats(
        items,
        |item| {
            let (resource, current) = get_resource_state(item)?;
            process_single(ctx, &resource, &current, opts)
        },
        move || cancelled.is_cancelled(),
    )?;
    Ok(stats.finish(ctx))
}
