//! Dependency-driven scheduling with a bounded pool of scoped task workers.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, mpsc};

use super::graph::ResolvedTaskGraph;
use crate::engine::task::{TaskExecution, TaskOutcome, task_entry};
use crate::engine::{self, Context, Task, TaskAssessment, TaskId};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{
    self, ActionCounts, BufferedLog, Log, LogEvent, Logger, Output as _, TaskStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BlockingOutcome {
    Blocked,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DependencySignal {
    Satisfied,
    Blocked {
        task: String,
        outcome: BlockingOutcome,
    },
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionSummary {
    failed_tasks: usize,
    tasks: HashMap<TaskId, TaskRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskRecord {
    name: String,
    outcome: TaskOutcome,
}

impl ExecutionSummary {
    /// Number of tasks whose own execution failed.
    #[must_use]
    pub(crate) const fn failure_count(&self) -> usize {
        self.failed_tasks
    }

    /// Whether any scheduled task was stopped by cancellation.
    #[must_use]
    pub(crate) fn was_interrupted(&self) -> bool {
        self.tasks
            .values()
            .any(|task| task.outcome == TaskOutcome::Cancelled)
    }

    /// Merge another execution phase into this summary.
    pub(crate) fn merge(&mut self, other: Self) {
        self.failed_tasks = self.failed_tasks.saturating_add(other.failed_tasks);
        self.tasks.extend(other.tasks);
    }

    /// Include failures detected before scheduler dispatch.
    pub(crate) const fn add_failures(&mut self, count: usize) {
        self.failed_tasks = self.failed_tasks.saturating_add(count);
    }

    /// Record one task's dependency outcome.
    pub(crate) fn record(
        &mut self,
        task_id: TaskId,
        name: impl Into<String>,
        outcome: TaskOutcome,
    ) {
        self.tasks.insert(
            task_id,
            TaskRecord {
                name: name.into(),
                outcome,
            },
        );
    }

    fn record_completion(&mut self, task: &dyn Task, execution: TaskExecution) {
        self.add_failures(usize::from(execution.status == TaskStatus::Failed));
        self.record(task.task_id(), task.name(), execution.outcome);
    }

    /// Look up an outcome recorded by this or an earlier phase.
    #[must_use]
    pub(crate) fn outcome(&self, task_id: &TaskId) -> Option<TaskOutcome> {
        self.tasks.get(task_id).map(|task| task.outcome)
    }

    fn task_name(&self, task_id: &TaskId) -> &str {
        self.tasks
            .get(task_id)
            .map_or("earlier task", |task| task.name.as_str())
    }
}

impl DependencySignal {
    fn blocked(task: impl Into<String>, outcome: BlockingOutcome) -> Self {
        Self::Blocked {
            task: task.into(),
            outcome,
        }
    }

    fn combine(self, other: Self) -> Self {
        match (self, other) {
            (
                Self::Blocked {
                    task: left_task,
                    outcome: left_outcome,
                },
                Self::Blocked {
                    task: right_task,
                    outcome: right_outcome,
                },
            ) => {
                let (outcome, task) = (left_outcome, left_task).max((right_outcome, right_task));
                Self::blocked(task, outcome)
            }
            (blocked @ Self::Blocked { .. }, _) | (_, blocked @ Self::Blocked { .. }) => blocked,
            (Self::Cancelled, _) | (_, Self::Cancelled) => Self::Cancelled,
            _ => Self::Satisfied,
        }
    }

    fn from_outcome(name: &str, outcome: TaskOutcome) -> Self {
        match outcome {
            TaskOutcome::Satisfied => Self::Satisfied,
            TaskOutcome::Unmet => Self::blocked(name, BlockingOutcome::Incomplete),
            TaskOutcome::Failed => Self::blocked(name, BlockingOutcome::Failed),
            TaskOutcome::Blocked => Self::blocked(name, BlockingOutcome::Blocked),
            TaskOutcome::Cancelled => Self::Cancelled,
        }
    }

    fn for_edge(self, blocks_on_failure: bool) -> Self {
        match self {
            Self::Blocked { .. } if !blocks_on_failure => Self::Satisfied,
            signal @ (Self::Satisfied | Self::Blocked { .. } | Self::Cancelled) => signal,
        }
    }

    fn reason(&self) -> Option<String> {
        match self {
            Self::Blocked { task, outcome } => {
                let state = match outcome {
                    BlockingOutcome::Blocked => "",
                    BlockingOutcome::Incomplete => "incomplete ",
                    BlockingOutcome::Failed => "failed ",
                };
                Some(format!("blocked by {state}dependency: {task}"))
            }
            Self::Satisfied | Self::Cancelled => None,
        }
    }
}

fn record_scheduler_skip(task: &dyn Task, log: &dyn Log, reason: &str, status: TaskStatus) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    log.debug(reason);
    log.record_task(task_entry(
        task,
        &task.log_key(),
        status,
        Some(reason),
        ActionCounts::default(),
    ));
}

/// Execute a single task, catching any panic.
///
/// Returns the recorded task status. On panic the task is recorded as
/// [`TaskStatus::Failed`], any buffered output is flushed, and dependents are
/// blocked the same way they are for ordinary task failures.
fn run_task_buffered(
    task: &dyn Task,
    assessment: &TaskAssessment,
    ctx: &Context,
    log: &Arc<Logger>,
    notify_start: bool,
) -> TaskExecution {
    if notify_start {
        log.notify_task_start(task.name());
    }
    let buf = Arc::new(BufferedLog::new(Arc::clone(log)));
    let buffered_log: Arc<dyn Log> = Arc::<BufferedLog>::clone(&buf);
    let task_ctx = ctx.with_log(buffered_log);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        engine::task::execute_assessed(task, assessment, &task_ctx)
    }));

    let execution = match result {
        Ok(execution) => execution,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| format!("task panicked: {s}"))
                .or_else(|| {
                    payload
                        .downcast_ref::<String>()
                        .map(|s| format!("task panicked: {s}"))
                })
                .unwrap_or_else(|| "task panicked".to_string());
            buf.error(format!("{}: {msg}", task.name()));
            log.record_task(task_entry(
                task,
                &task.log_key(),
                TaskStatus::Failed,
                Some(&msg),
                ActionCounts::default(),
            ));
            TaskExecution {
                status: TaskStatus::Failed,
                outcome: TaskOutcome::Failed,
            }
        }
    };

    buf.flush_and_complete(&task.log_key(), task.name());
    execution
}

/// Resolve one task against its dependency outcome, running it only when the
/// dependencies were satisfied and the run has not been cancelled.
///
/// Parallel runs announce task starts because output can interleave.
fn dispatch_task(
    task: &dyn Task,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
    dependencies: &DependencySignal,
    notify_start: bool,
) -> (DependencySignal, TaskExecution) {
    let skip_reason = match dependencies {
        DependencySignal::Blocked { .. } => dependencies
            .reason()
            .map(|reason| (reason, dependencies.clone())),
        DependencySignal::Cancelled => Some(("cancelled".to_string(), DependencySignal::Cancelled)),
        DependencySignal::Satisfied if ctx.is_cancelled() => {
            Some(("cancelled".to_string(), DependencySignal::Cancelled))
        }
        DependencySignal::Satisfied => None,
    };

    let Some((reason, signal)) = skip_reason else {
        let execution = run_task_buffered(
            task,
            assessments
                .get(&task.task_id())
                .unwrap_or(&TaskAssessment::applicable()),
            ctx,
            log,
            notify_start,
        );
        return (
            DependencySignal::from_outcome(task.name(), execution.outcome),
            execution,
        );
    };

    let task_id = task.log_key();
    let (status, outcome) = if signal == DependencySignal::Cancelled {
        (TaskStatus::Interrupted, TaskOutcome::Cancelled)
    } else {
        (TaskStatus::Blocked, TaskOutcome::Blocked)
    };
    record_scheduler_skip(task, &**log, &reason, status);
    log.mark_task_completed(&task_id);
    log.emit_task_result_and_redraw(&task_id);
    (signal, TaskExecution { status, outcome })
}

/// Run a resolved graph with one worker in sequential mode and a bounded
/// pool in parallel mode. Task workers stay separate from Rayon's resource pool.
pub(crate) fn run_tasks(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
    prior: Option<&ExecutionSummary>,
) -> ExecutionSummary {
    let worker_limit = if ctx.parallel() {
        std::thread::available_parallelism().map_or(2, |cpus| cpus.get().max(2))
    } else {
        1
    };
    run_tasks_with_limit(tasks, graph, assessments, ctx, log, prior, worker_limit)
}

struct PendingTask {
    remaining_dependencies: usize,
    signal: DependencySignal,
}

#[expect(
    clippy::expect_used,
    reason = "scoped workers report every dispatched task, including unwinds"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "the resolved graph and scheduler queues share indices into the task slice"
)]
fn run_tasks_with_limit(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
    prior: Option<&ExecutionSummary>,
    worker_limit: usize,
) -> ExecutionSummary {
    let mut ready = VecDeque::new();
    let mut pending: Vec<_> = tasks
        .iter()
        .enumerate()
        .map(|(idx, task)| {
            let dependencies = graph.dependencies(idx);
            if dependencies.is_empty() {
                ready.push_back(idx);
            }
            if ctx.parallel()
                && let Some(diag) = log.run_log()
            {
                let message = if dependencies.is_empty() {
                    "no deps, ready".to_owned()
                } else {
                    let names: Vec<_> = dependencies.iter().map(|&dep| tasks[dep].name()).collect();
                    format!("waiting for: {}", names.join(", "))
                };
                diag.emit_task(LogEvent::TaskWait, task.name(), &message);
            }
            PendingTask {
                remaining_dependencies: dependencies.len(),
                signal: prior_dependency_signal(*task, assessments.get(&task.task_id()), prior),
            }
        })
        .collect();

    let worker_count = worker_limit.min(tasks.len());
    let mut summary = ExecutionSummary::default();
    std::thread::scope(|scope| {
        let mut workers = Vec::with_capacity(worker_count);
        let (done, completed) = mpsc::channel();
        for worker in 0..worker_count {
            let (jobs, queue) = mpsc::channel::<(usize, DependencySignal)>();
            workers.push(jobs);
            let done = done.clone();
            scope.spawn(move || {
                for (idx, signal) in queue {
                    let task = tasks[idx];
                    // Report even a panic outside the task body's own panic handler,
                    // so the coordinator never waits for work the worker abandoned.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        logging::set_log_thread_name(&task.log_key());
                        dispatch_task(task, assessments, ctx, log, &signal, ctx.parallel())
                    }));
                    if done.send((worker, idx, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(done);

        let mut idle: Vec<_> = (0..worker_count).collect();
        for _ in tasks {
            // An idle slot represents a worker with no queued or running task.
            let dispatch_count = ready.len().min(idle.len());
            for worker in idle.drain(..dispatch_count) {
                let idx = ready.pop_front().expect("ready task was counted");
                let signal =
                    std::mem::replace(&mut pending[idx].signal, DependencySignal::Satisfied);
                workers[worker]
                    .send((idx, signal))
                    .expect("idle task worker is running");
            }

            assert_ne!(
                idle.len(),
                worker_count,
                "a resolved graph must have runnable work"
            );
            let (worker, idx, result) = completed
                .recv()
                .expect("dispatched task must report completion");
            idle.push(worker);
            let (signal, execution) =
                result.unwrap_or_else(|payload| std::panic::resume_unwind(payload));
            summary.record_completion(tasks[idx], execution);
            for &dependent in graph.dependents(idx) {
                let state = &mut pending[dependent];
                let incoming = signal
                    .clone()
                    .for_edge(graph.blocks_on_failure(dependent, idx));
                let previous = std::mem::replace(&mut state.signal, DependencySignal::Satisfied);
                state.signal = previous.combine(incoming);
                state.remaining_dependencies -= 1;
                if state.remaining_dependencies == 0 {
                    ready.push_back(dependent);
                }
            }
        }
    });
    summary
}

fn prior_dependency_signal(
    task: &dyn Task,
    assessment: Option<&TaskAssessment>,
    prior: Option<&ExecutionSummary>,
) -> DependencySignal {
    let Some(prior) = prior else {
        return DependencySignal::Satisfied;
    };
    task.dependencies()
        .iter()
        .map(|dependency| (dependency, true))
        .chain(
            task.ordering_dependencies()
                .iter()
                .filter(|id| !assessment.is_some_and(|state| state.ordering_satisfied(id)))
                .map(|dependency| (dependency, false)),
        )
        .filter_map(|(dependency, blocks_on_failure)| {
            prior.outcome(dependency).map(|outcome| {
                DependencySignal::from_outcome(prior.task_name(dependency), outcome)
                    .for_edge(blocks_on_failure)
            })
        })
        .fold(DependencySignal::Satisfied, DependencySignal::combine)
}

#[cfg(test)]
#[path = "tests/scheduler/mod.rs"]
mod tests;
