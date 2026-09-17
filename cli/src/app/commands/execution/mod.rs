//! Application task execution policy.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::engine::{Context, Task, TaskId};
use crate::infra::logging::{Logger, OutputExt as _};

use super::error::{CommandInterrupted, TaskFailures};
mod elevation;

use elevation::ElevationBroker;
#[cfg(test)]
use elevation::build_elevated_child_args;

type RestartCondition<'a> = Box<dyn FnOnce() -> bool + 'a>;
type RestartAction<'a> = Box<dyn FnOnce() + 'a>;

/// A complete application execution plan.
///
/// The plan separates task discovery from execution policy. A simple plan runs
/// one graph; a restart plan runs the dependency closure ending at a boundary
/// before deciding whether the current process can continue.
pub(crate) struct ExecutionPlan<'a> {
    tasks: Vec<&'a dyn Task>,
    restart: Option<RestartPlan<'a>>,
}

struct RestartPlan<'a> {
    boundary: TaskId,
    requested: RestartCondition<'a>,
    action: RestartAction<'a>,
}

impl<'a> ExecutionPlan<'a> {
    /// Build a single-graph plan.
    pub(crate) fn single(tasks: impl IntoIterator<Item = &'a dyn Task>) -> Self {
        Self {
            tasks: tasks.into_iter().collect(),
            restart: None,
        }
    }

    /// Build a plan that may restart after `boundary` completes.
    pub(crate) fn with_restart(
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        boundary: TaskId,
        requested: impl FnOnce() -> bool + 'a,
        action: impl FnOnce() + 'a,
    ) -> Self {
        Self {
            tasks: tasks.into_iter().collect(),
            restart: Some(RestartPlan {
                boundary,
                requested: Box::new(requested),
                action: Box::new(action),
            }),
        }
    }
}

/// Coordinates application execution phases around the generic task engine.
///
/// The engine owns graph validation and scheduling. This coordinator owns
/// application policy that spans graphs: visible progress totals, restart
/// boundaries, elevation preparation, and final run status.
#[derive(Debug)]
pub(crate) struct RunCoordinator<'a> {
    ctx: &'a Context,
    log: &'a Arc<Logger>,
}

impl<'a> RunCoordinator<'a> {
    /// Create a coordinator for one command run.
    pub(crate) const fn new(ctx: &'a Context, log: &'a Arc<Logger>) -> Self {
        Self { ctx, log }
    }

    /// Execute an application plan to completion.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails, tasks fail, or work is interrupted.
    pub(crate) fn execute(&self, mut plan: ExecutionPlan<'_>) -> Result<()> {
        self.log
            .add_task_total(visible_count(plan.tasks.iter().copied()));

        let summary = if let Some(restart) = plan.restart.take() {
            self.execute_with_restart(plan.tasks, restart)?
        } else {
            Some(run_task_graph(&mut plan.tasks, self.ctx, self.log, None)?)
        };

        summary.map_or(Ok(()), |summary| finish_run(self.ctx, self.log, &summary))
    }

    fn execute_with_restart(
        &self,
        tasks: Vec<&dyn Task>,
        restart: RestartPlan<'_>,
    ) -> Result<Option<crate::engine::scheduler::ExecutionSummary>> {
        let boundary_closure = dependency_closure(&tasks, restart.boundary.clone())?;
        let mut summary = crate::engine::scheduler::ExecutionSummary::default();

        if boundary_closure.is_empty() {
            let mut all_tasks = tasks;
            summary.merge(run_task_graph(&mut all_tasks, self.ctx, self.log, None)?);
        } else {
            let mut prefix = tasks
                .iter()
                .copied()
                .filter(|task| boundary_closure.contains(&task.task_id()))
                .collect::<Vec<_>>();
            summary.merge(run_task_graph(&mut prefix, self.ctx, self.log, None)?);

            if self.ctx.is_cancelled() || summary.was_interrupted() {
                return Ok(Some(summary));
            }
            let boundary_satisfied = matches!(
                summary.outcome(&restart.boundary),
                Some(crate::engine::TaskOutcome::Satisfied)
            );
            let restart_requested = (restart.requested)();
            if restart_requested {
                if boundary_satisfied && summary.failure_count() == 0 && !self.ctx.is_cancelled() {
                    (restart.action)();
                    return Ok(None);
                }
                return Ok(Some(summary));
            }
            let mut remaining = tasks
                .iter()
                .copied()
                .filter(|task| !boundary_closure.contains(&task.task_id()))
                .collect::<Vec<_>>();
            let next = run_task_graph(&mut remaining, self.ctx, self.log, Some(&summary))?;
            summary.merge(next);
        }

        Ok(Some(summary))
    }
}

/// Execute a dependency-driven task graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
#[cfg(test)]
pub(crate) fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    RunCoordinator::new(ctx, log).execute(ExecutionPlan::single(tasks))
}

/// Execute tasks and invoke a restart action after a dependency boundary.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
#[cfg(test)]
pub(crate) fn run_tasks_to_completion_with_restart<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    boundary: TaskId,
    requested: impl FnOnce() -> bool + 'a,
    action: impl FnOnce() + 'a,
) -> Result<()> {
    RunCoordinator::new(ctx, log).execute(ExecutionPlan::with_restart(
        tasks, boundary, requested, action,
    ))
}

/// Count the visible tasks scheduled for progress reporting.
///
/// Internal tasks appear in neither normal progress nor the run summary.
fn visible_count<'a>(tasks: impl IntoIterator<Item = &'a dyn Task>) -> usize {
    tasks
        .into_iter()
        .filter(|task| task.visibility().is_visible())
        .count()
}

fn dependency_closure(tasks: &[&dyn Task], boundary: TaskId) -> Result<HashSet<TaskId>> {
    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(tasks)?;
    if !graph.contains(&boundary) {
        return Ok(HashSet::new());
    }

    let mut closure = HashSet::from([boundary]);
    graph.extend_dependency_closure(
        &mut closure,
        crate::engine::graph::DependencyEdges::Blocking,
    );
    Ok(closure)
}

fn run_task_graph(
    tasks: &mut Vec<&dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    prior: Option<&crate::engine::scheduler::ExecutionSummary>,
) -> Result<crate::engine::scheduler::ExecutionSummary> {
    if tasks.is_empty() {
        return Ok(crate::engine::scheduler::ExecutionSummary::default());
    }

    let task_count = tasks.len();
    let mut graph = resolve_task_graph(tasks, log)?;

    let assessments = if ctx.is_cancelled() {
        HashMap::new()
    } else {
        tasks
            .iter()
            .map(|task| (task.task_id(), task.assess(ctx)))
            .collect::<HashMap<_, _>>()
    };
    let mut summary = if ctx.is_cancelled() {
        crate::engine::scheduler::ExecutionSummary::default()
    } else {
        ElevationBroker::new(ctx, log).prepare(tasks, &assessments, &graph)
    };

    if tasks.is_empty() {
        return Ok(summary);
    }

    if tasks.len() != task_count {
        graph = resolve_task_graph(tasks, log)?;
    }
    let mut combined_prior = prior.cloned().unwrap_or_default();
    combined_prior.merge(summary.clone());
    let scheduled = if ctx.parallel() {
        crate::engine::scheduler::run_tasks_parallel_with_prior(
            tasks,
            &graph,
            &assessments,
            ctx,
            log,
            Some(&combined_prior),
        )
    } else {
        crate::engine::scheduler::run_tasks_sequential_with_prior(
            tasks,
            &graph,
            &assessments,
            ctx,
            log,
            Some(&combined_prior),
        )
    };
    summary.merge(scheduled);
    Ok(summary)
}

fn resolve_task_graph(
    tasks: &[&dyn Task],
    log: &Logger,
) -> Result<crate::engine::graph::ResolvedTaskGraph> {
    crate::engine::graph::ResolvedTaskGraph::resolve(tasks).map_err(|error| {
        let message = format!("{error} detected in task graph");
        log.error(&message);
        anyhow!(message)
    })
}

fn finish_run(
    ctx: &Context,
    log: &Arc<Logger>,
    summary: &crate::engine::scheduler::ExecutionSummary,
) -> Result<()> {
    log.print_summary();
    let count = summary.failure_count();
    if count > 0 {
        return Err(TaskFailures::new(count).into());
    }
    if ctx.is_cancelled() || summary.was_interrupted() {
        return Err(CommandInterrupted.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
