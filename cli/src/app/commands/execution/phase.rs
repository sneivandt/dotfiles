//! Matching scheduler inputs after applicability and elevation preparation.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use crate::engine::graph::ResolvedTaskGraph;
use crate::engine::scheduler::{self, ExecutionSummary};
use crate::engine::{Context, Task, TaskAssessment, TaskId};
use crate::infra::logging::Logger;

use super::elevation::ElevationBroker;
use super::resolve_task_graph;

/// Owns one validated task ordering, its indexed graph and cached assessments.
/// Scheduler inputs cannot be changed independently after preparation.
pub(super) struct PreparedPhase<'a> {
    tasks: Vec<&'a dyn Task>,
    graph: ResolvedTaskGraph,
    assessments: HashMap<TaskId, TaskAssessment>,
}

impl<'a> PreparedPhase<'a> {
    pub(super) fn prepare(
        mut tasks: Vec<&'a dyn Task>,
        ctx: &Context,
        log: &Arc<Logger>,
    ) -> Result<(Self, ExecutionSummary)> {
        let task_count = tasks.len();
        let mut graph = resolve_task_graph(&tasks, log)?;
        let mut assessments = if ctx.is_cancelled() {
            HashMap::new()
        } else {
            tasks
                .iter()
                .map(|task| (task.task_id(), task.assess(ctx)))
                .collect()
        };
        let summary = if ctx.is_cancelled() {
            ExecutionSummary::default()
        } else {
            ElevationBroker::new(ctx, log).prepare(&mut tasks, &assessments, &graph)
        };
        if tasks.len() != task_count {
            graph = resolve_task_graph(&tasks, log)?;
            assessments.retain(|id, _| graph.contains(id));
        }
        Ok((
            Self {
                tasks,
                graph,
                assessments,
            },
            summary,
        ))
    }

    pub(super) fn run(
        self,
        ctx: &Context,
        log: &Arc<Logger>,
        prior: &ExecutionSummary,
    ) -> ExecutionSummary {
        if ctx.parallel() {
            scheduler::run_tasks_parallel_with_prior(
                &self.tasks,
                &self.graph,
                &self.assessments,
                ctx,
                log,
                Some(prior),
            )
        } else {
            scheduler::run_tasks_sequential_with_prior(
                &self.tasks,
                &self.graph,
                &self.assessments,
                ctx,
                log,
                Some(prior),
            )
        }
    }
}
