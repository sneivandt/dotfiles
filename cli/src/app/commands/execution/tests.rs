use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::elevation::ElevationBroker;

use crate::engine::{TaskAssessment, TaskMeta, TaskOutcome, TaskResult};
use crate::test_helpers::{empty_config, make_static_context, numeric_task_id};

use super::*;

#[test]
fn progress_counts_visible_tasks_across_the_plan() {
    crate::test_helpers::test_task! {
        FirstVisible { name: "First visible", }
        SecondVisible { name: "Second visible", }
        Internal {
            name: "Internal step",
            visibility: crate::engine::TaskVisibility::Internal,
        }
    }
    let internal: [&dyn Task; 1] = [&Internal];
    let mixed: [&dyn Task; 3] = [&FirstVisible, &Internal, &SecondVisible];
    assert_eq!(visible_count(internal), 0);
    assert_eq!(visible_count(mixed), 2);
}

/// Shared ordered record of which tasks ran.
type Trace = Arc<Mutex<Vec<String>>>;

fn blocked_dependents<'task>(
    tasks: &[&dyn Task],
    roots: &HashMap<TaskId, &'task str>,
) -> HashMap<TaskId, &'task str> {
    crate::engine::graph::ResolvedTaskGraph::resolve(tasks)
        .expect("valid task graph")
        .blocked_dependents(roots)
}

fn trace() -> Trace {
    Arc::new(Mutex::new(Vec::new()))
}

#[derive(Debug, Clone, Copy)]
enum ProbeFailure {
    Error,
    Result,
}

/// A task with a runtime-supplied identity, dependency list, and outcome.
///
/// [`TaskId::dynamic`] lets a test build an arbitrary graph shape, which
/// type-derived ids cannot express.
struct ProbeTask {
    name: String,
    id: TaskId,
    dependencies: Vec<TaskId>,
    trace: Trace,
    failure: Option<ProbeFailure>,
    cancels: bool,
    interrupts: bool,
    assessments: AtomicUsize,
    requires_elevation: bool,
}

impl ProbeTask {
    fn new(name: &str, id: u64, trace: &Trace) -> Self {
        Self {
            name: name.to_string(),
            id: numeric_task_id(id),
            dependencies: Vec::new(),
            trace: Arc::clone(trace),
            failure: None,
            cancels: false,
            interrupts: false,
            assessments: AtomicUsize::new(0),
            requires_elevation: false,
        }
    }

    fn depends_on(mut self, ids: &[u64]) -> Self {
        self.dependencies = ids.iter().copied().map(numeric_task_id).collect();
        self
    }

    const fn failing(mut self) -> Self {
        self.failure = Some(ProbeFailure::Error);
        self
    }

    const fn cancelling(mut self) -> Self {
        self.cancels = true;
        self
    }

    const fn interrupted(mut self) -> Self {
        self.interrupts = true;
        self
    }
}

impl Task for ProbeTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(&self.name)
    }

    fn task_id(&self) -> TaskId {
        self.id.clone()
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.dependencies
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn assess(
        &self,
        _ctx: &Context,
        _inputs: &mut crate::engine::AssessmentInputs<'_>,
    ) -> TaskAssessment {
        self.assessments.fetch_add(1, Ordering::SeqCst);
        TaskAssessment::applicable().with_elevation(self.requires_elevation)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(self.name.clone());
        if self.cancels {
            ctx.cancellation_token().cancel();
        }
        if let Some(failure) = self.failure {
            let message = format!("{} failed", self.name);
            return match failure {
                ProbeFailure::Error => Err(anyhow!(message)),
                ProbeFailure::Result => Ok(TaskResult::Failed(message)),
            };
        }
        if self.interrupts {
            return Err(cancelled_command().into());
        }
        Ok(TaskResult::Ok)
    }
}

fn cancelled_command() -> crate::infra::exec::ExecError {
    crate::infra::exec::ExecError::Cancelled {
        command: "fixture command".into(),
        result: crate::infra::exec::ExecResult::success(""),
    }
}

fn execution_context(parallel: bool) -> (Context, Arc<Logger>) {
    let (ctx, log) = make_static_context("fixture-root".into(), None);
    (ctx.with_parallel(parallel), log)
}

struct ExecutionFixture {
    trace: Trace,
    ctx: Context,
    log: Arc<Logger>,
}

impl ExecutionFixture {
    fn with_parallel(parallel: bool) -> Self {
        let (ctx, log) = execution_context(parallel);
        Self {
            trace: trace(),
            ctx,
            log,
        }
    }

    fn task(&self, name: &str, id: u64) -> ProbeTask {
        ProbeTask::new(name, id, &self.trace)
    }

    fn entries(&self) -> Vec<String> {
        self.trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn run(&self, tasks: &[&dyn Task]) -> Result<()> {
        run_tasks_to_completion(tasks.iter().copied(), &self.ctx, &self.log)
    }

    fn run_with_restart<'a>(
        &self,
        tasks: &[&'a dyn Task],
        boundary: TaskId,
        requested: impl FnOnce() -> bool + 'a,
        action: impl FnOnce() + 'a,
    ) -> Result<()> {
        run_tasks_to_completion_with_restart(
            tasks.iter().copied(),
            &self.ctx,
            &self.log,
            boundary,
            requested,
            action,
        )
    }
}

mod completion;
mod elevation;
mod planning;
mod restart;
