//! Top-level resource orchestration: check state, dispatch to sequential or
//! parallel processing, and collect stats.

use anyhow::Result;

use super::apply;
use super::context::Context;
use super::mode::ProcessOpts;
use super::parallel;
use super::stats::{TaskResult, TaskStats};
use crate::resources::{Applicable, Resource, ResourceState};

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
        parallel::process_resources_parallel(ctx, resources, opts)
    } else {
        let mut stats = TaskStats::new();
        for resource in resources {
            if ctx.is_cancelled() {
                ctx.log.warn("cancelled — stopping before next resource");
                break;
            }
            let current = resource.current_state()?;
            stats += apply::process_single(ctx, &resource, &current, opts)?;
        }
        Ok(stats.finish(ctx))
    }
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
        parallel::process_resource_states_parallel(ctx, resource_states, opts)
    } else {
        let mut stats = TaskStats::new();
        for (resource, current) in resource_states {
            if ctx.is_cancelled() {
                ctx.log.warn("cancelled — stopping before next resource");
                break;
            }
            stats += apply::process_single(ctx, &resource, &current, opts)?;
        }
        Ok(stats.finish(ctx))
    }
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
        parallel::process_remove_parallel(ctx, resources, verb)
    } else {
        let mut stats = TaskStats::new();
        for resource in resources {
            if ctx.is_cancelled() {
                ctx.log.warn("cancelled — stopping before next resource");
                break;
            }
            let current = resource.current_state()?;
            stats += apply::remove_single(ctx, &resource, &current, verb)?;
        }
        Ok(stats.finish(ctx))
    }
}
