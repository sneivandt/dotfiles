//! Phased application task execution policy.

use std::sync::Arc;

use anyhow::Result;

use crate::engine::{Context, Task, TaskPhase};
use crate::infra::logging::Logger;

use super::error::TaskFailures;

type LateTaskProvider<'a> = Box<dyn FnOnce() -> Vec<Box<dyn Task>> + 'a>;

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

/// Execute the full phased task pipeline.
///
/// Phases run strictly in order, each completing before the next begins.
/// Within a phase, tasks run as soon as their dependencies complete.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
pub(crate) fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    run_tasks_to_completion_inner(tasks, ctx, log, None)
}

/// Execute the phased task pipeline and inject additional tasks after a phase.
///
/// The provider runs exactly once after `after_phase` completes. Injected tasks
/// then flow through the normal graph resolution, elevation, and execution path
/// for their own phases.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
pub(crate) fn run_tasks_to_completion_with_late_tasks<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    after_phase: TaskPhase,
    provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
) -> Result<()> {
    run_tasks_to_completion_inner(tasks, ctx, log, Some((after_phase, Box::new(provider))))
}

fn run_tasks_to_completion_inner<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    mut late_tasks: Option<(TaskPhase, LateTaskProvider<'a>)>,
) -> Result<()> {
    let mut owned_late_tasks: Vec<Box<dyn Task>> = Vec::new();
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();
    let phases = [
        TaskPhase::Bootstrap,
        TaskPhase::Sync,
        TaskPhase::Provision,
        TaskPhase::Validation,
        TaskPhase::Update,
    ];

    for phase in phases {
        if ctx.is_cancelled() {
            log.warn("cancelled - stopping before next phase");
            break;
        }

        let mut phase_tasks: Vec<&dyn Task> = tasks
            .iter()
            .copied()
            .chain(owned_late_tasks.iter().map(Box::as_ref))
            .filter(|task| task.phase() == phase)
            .collect();

        if !phase_tasks.is_empty() {
            let sudo_task_names: Vec<&str> =
                if ctx.parallel() && !ctx.dry_run() && phase_tasks.len() > 1 {
                    phase_tasks
                        .iter()
                        .filter(|task| task.requires_elevation(ctx))
                        .map(|task| task.name())
                        .collect()
                } else {
                    Vec::new()
                };
            let sudo_failed =
                !sudo_task_names.is_empty() && !prime_sudo(ctx, log, &sudo_task_names);

            if sudo_failed {
                if ctx.is_cancelled() {
                    log.warn("cancelled - stopping before next phase");
                    break;
                }

                let reason = "sudo credentials unavailable";
                phase_tasks.retain(|task| {
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

            if !phase_tasks.is_empty() {
                let graph = match crate::engine::graph::ResolvedTaskGraph::resolve(&phase_tasks) {
                    Ok(graph) => graph,
                    Err(error) => {
                        let message = format!("{error} detected in {phase} phase task graph");
                        log.error(&message);
                        anyhow::bail!(message);
                    }
                };

                if ctx.parallel() {
                    crate::engine::scheduler::run_tasks_parallel(&phase_tasks, &graph, ctx, log);
                } else {
                    crate::engine::scheduler::run_tasks_sequential(&phase_tasks, &graph, ctx, log);
                }
            }
        }

        if late_tasks
            .as_ref()
            .is_some_and(|(after_phase, _)| *after_phase == phase)
            && let Some((_, provider)) = late_tasks.take()
        {
            owned_late_tasks = provider();
        }
    }

    log.print_summary();

    let count = log.failure_count();
    if count > 0 {
        return Err(TaskFailures::new(count).into());
    }
    Ok(())
}
