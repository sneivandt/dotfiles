//! Rayon-based parallel resource processing.

use std::sync::Mutex;

use anyhow::Result;

use super::apply::{process_single, remove_single};
use super::{ProcessOpts, TaskStats};
use crate::logging::{diag_thread_name, set_diag_thread_name};
use crate::resources::{Resource, ResourceState};
use crate::tasks::Context;

/// Process resources in parallel using Rayon.
pub(super) fn process_resources_parallel<R: Resource + Send>(
    ctx: &Context,
    resources: Vec<R>,
    opts: &ProcessOpts,
) -> Result<super::TaskResult> {
    run_parallel(ctx, resources, opts, |resource| {
        let state = resource.current_state()?;
        Ok((resource, state))
    })
}

/// Process resources with pre-computed states in parallel using Rayon.
pub(super) fn process_resource_states_parallel<R: Resource + Send>(
    ctx: &Context,
    resource_states: Vec<(R, ResourceState)>,
    opts: &ProcessOpts,
) -> Result<super::TaskResult> {
    run_parallel(ctx, resource_states, opts, Ok)
}

/// Remove resources in parallel using Rayon.
pub(super) fn process_remove_parallel<R: Resource + Send>(
    ctx: &Context,
    resources: Vec<R>,
    verb: &str,
) -> Result<super::TaskResult> {
    let stats = collect_parallel_stats(resources, |resource| {
        let current = resource.current_state()?;
        remove_single(ctx, &resource, &current, verb)
    })?;
    Ok(stats.finish(ctx))
}

/// Accumulate per-item [`TaskStats`] deltas in parallel using Rayon.
///
/// Runs `work` on each item concurrently; the resulting deltas are added to a
/// shared `Mutex<TaskStats>`. The accumulated total is returned when all items
/// have been processed.
///
/// The diagnostic thread name is captured once before dispatching and re-set
/// on each iteration so the log timeline remains accurate even when Rayon
/// reuses threads across work items (a stale name is harmless but misleading).
fn collect_parallel_stats<T: Send>(
    items: Vec<T>,
    work: impl Fn(T) -> Result<TaskStats> + Sync + Send,
) -> Result<TaskStats> {
    use rayon::prelude::*;
    let task_name = diag_thread_name();
    let stats = Mutex::new(TaskStats::new());
    items.into_par_iter().try_for_each(|item| -> Result<()> {
        set_diag_thread_name(&task_name);
        let delta = work(item)?;
        *stats
            .lock()
            .map_err(|e| anyhow::anyhow!("stats mutex poisoned: {e}"))? += delta;
        Ok(())
    })?;
    Ok(stats
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Generic parallel processing helper using Rayon.
///
/// Accepts a vector of items and a closure that extracts a `(Resource, ResourceState)`
/// pair from each item. The closure runs in parallel; stats are synchronized via a mutex.
///
/// The per-item work (`get_resource_state` and `process_single`) runs **without** the
/// stats lock held, so all resources can be applied concurrently. The lock is acquired
/// only for the brief stats counter update after the work completes.
fn run_parallel<T: Send, R: Resource + Send>(
    ctx: &Context,
    items: Vec<T>,
    opts: &ProcessOpts,
    get_resource_state: impl Fn(T) -> Result<(R, ResourceState)> + Sync,
) -> Result<super::TaskResult> {
    let stats = collect_parallel_stats(items, |item| {
        let (resource, current) = get_resource_state(item)?;
        process_single(ctx, &resource, current, opts)
    })?;
    Ok(stats.finish(ctx))
}
