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
        prior: Option<&ExecutionSummary>,
    ) -> Result<(Self, ExecutionSummary)> {
        let task_count = tasks.len();
        let declared = resolve_task_graph(&tasks, log)?;
        let has_provider =
            |id: &TaskId| declared.contains(id) || prior.is_some_and(|s| s.outcome(id).is_some());
        let mut inputs = crate::engine::AssessmentInputs::new(&has_provider);
        let mut assessments = if ctx.is_cancelled() {
            HashMap::new()
        } else {
            tasks
                .iter()
                .map(|task| (task.task_id(), task.assess(ctx, &mut inputs)))
                .collect()
        };
        let mut graph = ResolvedTaskGraph::resolve_assessed(&tasks, &assessments)?;
        let summary = if ctx.is_cancelled() {
            ExecutionSummary::default()
        } else {
            ElevationBroker::new(ctx, log).prepare(&mut tasks, &assessments, &graph)
        };
        if tasks.len() != task_count {
            graph = ResolvedTaskGraph::resolve_assessed(&tasks, &assessments)?;
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
        scheduler::run_tasks(
            &self.tasks,
            &self.graph,
            &self.assessments,
            ctx,
            log,
            Some(prior),
        )
    }
}
