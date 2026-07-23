//! Application task execution policy.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::engine::{Context, Task, TaskId};
use crate::infra::logging::Logger;

use super::error::TaskFailures;

#[cfg(unix)]
fn prime_sudo(ctx: &Context, log: &Arc<Logger>, task_names: &[&str]) -> bool {
    if !crate::infra::elevation::sudo_available(ctx.executor()) {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return false;
    }
    log.debug("priming sudo credential cache");

    if crate::infra::elevation::sudo_credentials_cached() {
        log.debug("sudo credentials already cached");
        return true;
    }

    log.separate_from_startup();
    log.always(&format!("sudo is required for: {}", task_names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match crate::infra::elevation::prime_sudo_credentials() {
        Ok(true) => true,
        Ok(false) => {
            log.separate_from_startup();
            log.error("sudo credential priming failed");
            false
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(&format!("failed to run sudo: {error:#}"));
            false
        }
    }
}

#[cfg(not(unix))]
const fn prime_sudo(_ctx: &Context, _log: &Arc<Logger>, _task_names: &[&str]) -> bool {
    true
}

/// Execute a dependency-driven task graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
pub(crate) fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let mut tasks = tasks.into_iter().collect::<Vec<_>>();
    run_task_graph(&mut tasks, ctx, log)?;
    finish_run(log)
}

/// Execute tasks and inject additional tasks after a dependency boundary.
///
/// When `boundary` is present, its complete dependency closure runs first. The
/// provider then observes any state refreshed by that closure, and its tasks
/// join the remaining static tasks in a second dependency graph. If the
/// boundary was filtered out, the provider runs before the single graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
pub(crate) fn run_tasks_to_completion_with_late_tasks<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    boundary: TaskId,
    provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
) -> Result<()> {
    let tasks = tasks.into_iter().collect::<Vec<_>>();
    let boundary_closure = dependency_closure(&tasks, boundary);

    if boundary_closure.is_empty() {
        let late_tasks = provider();
        let mut all_tasks = tasks;
        all_tasks.extend(late_tasks.iter().map(Box::as_ref));
        run_task_graph(&mut all_tasks, ctx, log)?;
    } else {
        let mut prefix = tasks
            .iter()
            .copied()
            .filter(|task| boundary_closure.contains(&task.task_id()))
            .collect::<Vec<_>>();
        run_task_graph(&mut prefix, ctx, log)?;

        if log.failure_count() == 0 && !ctx.is_cancelled() {
            let late_tasks = provider();
            let mut remaining = tasks
                .iter()
                .copied()
                .filter(|task| !boundary_closure.contains(&task.task_id()))
                .collect::<Vec<_>>();
            remaining.extend(late_tasks.iter().map(Box::as_ref));
            run_task_graph(&mut remaining, ctx, log)?;
        }
    }

    finish_run(log)
}

fn dependency_closure(tasks: &[&dyn Task], boundary: TaskId) -> HashSet<TaskId> {
    let by_id = tasks
        .iter()
        .map(|task| (task.task_id(), *task))
        .collect::<std::collections::HashMap<_, _>>();
    if !by_id.contains_key(&boundary) {
        return HashSet::new();
    }

    let mut closure = HashSet::from([boundary]);
    let mut pending = vec![boundary];
    while let Some(id) = pending.pop() {
        if let Some(task) = by_id.get(&id) {
            for dependency in task.dependencies() {
                if by_id.contains_key(dependency) && closure.insert(*dependency) {
                    pending.push(*dependency);
                }
            }
        }
    }
    closure
}

fn run_task_graph(tasks: &mut Vec<&dyn Task>, ctx: &Context, log: &Arc<Logger>) -> Result<()> {
    if ctx.is_cancelled() || tasks.is_empty() {
        return Ok(());
    }

    let sudo_task_names: Vec<&str> = if ctx.parallel() && !ctx.dry_run() && tasks.len() > 1 {
        tasks
            .iter()
            .filter(|task| task.requires_elevation(ctx))
            .map(|task| task.name())
            .collect()
    } else {
        Vec::new()
    };
    if !sudo_task_names.is_empty() && !prime_sudo(ctx, log, &sudo_task_names) {
        let reason = "sudo credentials unavailable";
        tasks.retain(|task| {
            if task.requires_elevation(ctx) {
                let span = tracing::info_span!("task", name = task.name());
                let _enter = span.enter();
                log.debug(reason);
                log.record_task(
                    task.name(),
                    crate::infra::logging::TaskStatus::Skipped,
                    Some(reason),
                );
                log.emit_task_result_and_redraw(task.name());
                false
            } else {
                true
            }
        });
    }

    if tasks.is_empty() {
        return Ok(());
    }

    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(tasks).map_err(|error| {
        let message = format!("{error} detected in task graph");
        log.error(&message);
        anyhow!(message)
    })?;
    if ctx.parallel() {
        crate::engine::scheduler::run_tasks_parallel(tasks, &graph, ctx, log);
    } else {
        crate::engine::scheduler::run_tasks_sequential(tasks, &graph, ctx, log);
    }
    Ok(())
}

fn finish_run(log: &Arc<Logger>) -> Result<()> {
    log.print_summary();
    let count = log.failure_count();
    if count > 0 {
        return Err(TaskFailures::new(count).into());
    }
    Ok(())
}
