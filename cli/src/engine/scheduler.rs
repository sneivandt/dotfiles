//! Dependency-driven parallel task scheduling.
//!
//! Provides [`run_tasks_parallel`](crate::engine::scheduler::run_tasks_parallel) for executing tasks concurrently using OS
//! threads.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};

use super::graph::ResolvedTaskGraph;
use crate::engine::task::{TaskDisposition, TaskExecution};
use crate::engine::{self, Context, Task, TaskAssessment, TaskId};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{
    self, ActionCounts, BufferedLog, Log, LogEvent, Logger, Output as _, TaskEntry, TaskStatus,
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

/// Execution facts returned to application policy independently of logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskOutcome {
    Satisfied,
    Unmet,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionSummary {
    failed_tasks: usize,
    tasks: HashMap<TaskId, TaskRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskRecord {
    selector: String,
    name: String,
    outcome: TaskOutcome,
}

impl ExecutionSummary {
    /// Number of tasks whose own execution failed.
    #[must_use]
    pub(crate) const fn failure_count(&self) -> usize {
        self.failed_tasks
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
        selector: impl Into<String>,
        name: impl Into<String>,
        outcome: TaskOutcome,
    ) {
        self.tasks.insert(
            task_id,
            TaskRecord {
                selector: selector.into(),
                name: name.into(),
                outcome,
            },
        );
    }

    /// Look up an outcome recorded by this or an earlier phase.
    #[must_use]
    pub(crate) fn outcome(&self, task_id: &TaskId) -> Option<TaskOutcome> {
        self.tasks.get(task_id).map(|task| task.outcome)
    }

    fn task_name(&self, task_id: &TaskId) -> String {
        self.tasks
            .get(task_id)
            .map_or_else(|| "earlier task".to_string(), |task| task.name.clone())
    }

    /// Selectors that should be retried after this run.
    #[must_use]
    pub(crate) fn incomplete_selectors(&self) -> HashSet<String> {
        self.tasks
            .values()
            .filter(|task| {
                matches!(
                    task.outcome,
                    TaskOutcome::Unmet | TaskOutcome::Failed | TaskOutcome::Blocked
                )
            })
            .map(|task| task.selector.clone())
            .collect()
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
                if (right_outcome, &right_task) > (left_outcome, &left_task) {
                    Self::blocked(right_task, right_outcome)
                } else {
                    Self::blocked(left_task, left_outcome)
                }
            }
            (blocked @ Self::Blocked { .. }, _) | (_, blocked @ Self::Blocked { .. }) => blocked,
            (Self::Cancelled, _) | (_, Self::Cancelled) => Self::Cancelled,
            _ => Self::Satisfied,
        }
    }

    fn from_task(task: &dyn Task, disposition: TaskDisposition) -> Self {
        match disposition {
            TaskDisposition::Satisfied => Self::Satisfied,
            TaskDisposition::Unmet => Self::blocked(task.name(), BlockingOutcome::Incomplete),
            TaskDisposition::Failed => Self::blocked(task.name(), BlockingOutcome::Failed),
            TaskDisposition::Cancelled => Self::Cancelled,
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

impl From<TaskDisposition> for TaskOutcome {
    fn from(disposition: TaskDisposition) -> Self {
        match disposition {
            TaskDisposition::Satisfied => Self::Satisfied,
            TaskDisposition::Unmet => Self::Unmet,
            TaskDisposition::Failed => Self::Failed,
            TaskDisposition::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug)]
struct TaskRuntime {
    dependency_receiver: Option<mpsc::Receiver<DependencySignal>>,
    dependency_sender: Option<mpsc::Sender<DependencySignal>>,
    dependent_senders: Vec<(mpsc::Sender<DependencySignal>, bool)>,
}

impl TaskRuntime {
    fn new(has_dependencies: bool) -> Self {
        let (dependency_sender, dependency_receiver) = if has_dependencies {
            let (tx, rx) = mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        Self {
            dependency_receiver,
            dependency_sender,
            dependent_senders: Vec::new(),
        }
    }
}

fn signal_dependents(
    task_name: &str,
    senders: Vec<(mpsc::Sender<DependencySignal>, bool)>,
    signal: &DependencySignal,
) {
    for (tx, blocks_on_failure) in senders {
        let delivered = if !blocks_on_failure && matches!(signal, DependencySignal::Blocked { .. })
        {
            DependencySignal::Satisfied
        } else {
            signal.clone()
        };
        if tx.send(delivered).is_err() {
            tracing::debug!(
                "dependent task channel closed before {task_name} signalled completion"
            );
        }
    }
}

fn dependency_outcome(
    receiver: Option<mpsc::Receiver<DependencySignal>>,
    dependency_count: usize,
) -> DependencySignal {
    let Some(receiver) = receiver else {
        return DependencySignal::Satisfied;
    };

    (0..dependency_count).fold(DependencySignal::Satisfied, |outcome, _| {
        let signal = receiver.recv().unwrap_or_else(|_| {
            DependencySignal::blocked("dependency task", BlockingOutcome::Blocked)
        });
        outcome.combine(signal)
    })
}

fn record_scheduler_skip(task: &dyn Task, log: &dyn Log, reason: &str) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    log.run_task_event(LogEvent::TaskSkip, task.name(), reason);
    log.debug(reason);
    log.record_task(TaskEntry::new(
        task.task_id().record_key(),
        task.name(),
        TaskStatus::Skipped,
        Some(reason),
        ActionCounts::default(),
        task.visibility(),
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
            log.run_task_event(LogEvent::TaskFail, task.name(), &msg);
            buf.error(format!("{}: {msg}", task.name()));
            log.record_task(TaskEntry::new(
                task.task_id().record_key(),
                task.name(),
                TaskStatus::Failed,
                Some(&msg),
                ActionCounts::default(),
                task.visibility(),
            ));
            TaskExecution {
                status: TaskStatus::Failed,
                disposition: TaskDisposition::Failed,
            }
        }
    };

    buf.flush_and_complete(&task.task_id().record_key(), task.name(), execution.status);
    execution
}

/// Resolve one task against its dependency outcome, running it only when the
/// dependencies were satisfied and the run has not been cancelled.
///
/// Shared by both schedulers so the parallel and sequential paths cannot drift:
/// they differ only in whether a task start notification is emitted, which the
/// sequential path does not need because it never interleaves output.
fn dispatch_task(
    task: &dyn Task,
    assessment: &TaskAssessment,
    ctx: &Context,
    log: &Arc<Logger>,
    dependencies: &DependencySignal,
    notify_start: bool,
) -> (DependencySignal, TaskStatus, TaskOutcome) {
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
        let execution = run_task_buffered(task, assessment, ctx, log, notify_start);
        return (
            DependencySignal::from_task(task, execution.disposition),
            execution.status,
            execution.disposition.into(),
        );
    };

    let task_id = task.task_id().record_key();
    record_scheduler_skip(task, &**log, &reason);
    log.mark_task_completed(&task_id);
    log.emit_task_result_and_redraw(&task_id);
    let outcome = if signal == DependencySignal::Cancelled {
        TaskOutcome::Cancelled
    } else {
        TaskOutcome::Blocked
    };
    (signal, TaskStatus::Skipped, outcome)
}

/// Run tasks in parallel using a dependency graph.
///
/// Each task is spawned into an OS thread (via `std::thread::scope`) and waits
/// for its dependencies to complete before executing.  OS threads are used
/// deliberately — blocking on an `mpsc` channel inside a Rayon worker would
/// exhaust Rayon's fixed-size thread pool and deadlock when the pool is smaller
/// than the number of tasks with unsatisfied dependencies (common on 2-vCPU CI
/// runners).  Output is buffered per-task and flushed to the console
/// immediately on completion.
#[cfg(test)]
pub(crate) fn run_tasks_parallel(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> ExecutionSummary {
    run_tasks_parallel_with_prior(tasks, graph, assessments, ctx, log, None)
}

pub(crate) fn run_tasks_parallel_with_prior(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
    prior: Option<&ExecutionSummary>,
) -> ExecutionSummary {
    let mut runtimes: Vec<TaskRuntime> = (0..tasks.len())
        .map(|task_idx| TaskRuntime::new(!graph.dependencies(task_idx).is_empty()))
        .collect();

    for dep_idx in 0..tasks.len() {
        for &dependent_idx in graph.dependents(dep_idx) {
            let dependency_sender = runtimes
                .get(dependent_idx)
                .and_then(|runtime| runtime.dependency_sender.as_ref())
                .cloned();
            if let Some(tx) = dependency_sender
                && let Some(runtime) = runtimes.get_mut(dep_idx)
            {
                runtime
                    .dependent_senders
                    .push((tx, graph.blocks_on_failure(dependent_idx, dep_idx)));
            }
        }
    }

    // Drop the original senders so an unexpected panic before signalling closes
    // dependent receivers instead of leaving them blocked forever.
    for runtime in &mut runtimes {
        runtime.dependency_sender = None;
    }

    let recorded_outcomes = std::sync::Mutex::new(HashMap::new());
    let failed_tasks = std::sync::Mutex::new(0_usize);
    std::thread::scope(|s| {
        for (idx, (task, runtime)) in tasks.iter().zip(runtimes.iter_mut()).enumerate() {
            let task = *task;
            let dependency_receiver = runtime.dependency_receiver.take();
            let dependent_senders = std::mem::take(&mut runtime.dependent_senders);
            let dep_names: Vec<&str> = graph
                .dependencies(idx)
                .iter()
                .filter_map(|&dep_idx| tasks.get(dep_idx).map(|dep_task| dep_task.name()))
                .collect();
            let dep_count = dep_names.len();
            let failed_tasks = &failed_tasks;
            let recorded_outcomes = &recorded_outcomes;
            let prior_signal = prior_dependency_signal(task, prior);

            s.spawn(move || {
                logging::set_log_thread_name(task.name());

                if let Some(diag) = log.run_log() {
                    if dep_names.is_empty() {
                        diag.emit_task(LogEvent::TaskWait, task.name(), "no deps, ready");
                    } else {
                        diag.emit_task(
                            LogEvent::TaskWait,
                            task.name(),
                            &format!("waiting for: {}", dep_names.join(", ")),
                        );
                    }
                }

                // Wait for all deps: receive one outcome per dependency.
                // Receive every signal so failure takes precedence over
                // cancellation when dependency outcomes are mixed.
                let dependency_signal =
                    dependency_outcome(dependency_receiver, dep_count).combine(prior_signal);
                let assessment = assessments
                    .get(&task.task_id())
                    .cloned()
                    .unwrap_or_else(TaskAssessment::applicable);
                let (signal, status, outcome) =
                    dispatch_task(task, &assessment, ctx, log, &dependency_signal, true);
                if status == TaskStatus::Failed {
                    let mut count = failed_tasks
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *count = count.saturating_add(1);
                }
                recorded_outcomes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(
                        task.task_id(),
                        (
                            task.selector().to_string(),
                            task.name().to_string(),
                            outcome,
                        ),
                    );

                signal_dependents(task.name(), dependent_senders, &signal);
            });
        }
    });
    let failed_count = *failed_tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let recorded = recorded_outcomes
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut task_records = HashMap::with_capacity(recorded.len());
    for (task_id, (selector, name, outcome)) in recorded {
        task_records.insert(
            task_id,
            TaskRecord {
                selector,
                name,
                outcome,
            },
        );
    }
    ExecutionSummary {
        failed_tasks: failed_count,
        tasks: task_records,
    }
}

/// Run tasks sequentially in dependency-safe order.
///
/// Normal task failures block dependent tasks just like the parallel scheduler;
/// deliberate skips and not-applicable outcomes still satisfy dependencies.
#[cfg(test)]
pub(crate) fn run_tasks_sequential(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> ExecutionSummary {
    run_tasks_sequential_with_prior(tasks, graph, assessments, ctx, log, None)
}

pub(crate) fn run_tasks_sequential_with_prior(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    assessments: &HashMap<TaskId, TaskAssessment>,
    ctx: &Context,
    log: &Arc<Logger>,
    prior: Option<&ExecutionSummary>,
) -> ExecutionSummary {
    let mut signals: Vec<Option<DependencySignal>> = vec![None; tasks.len()];
    let mut summary = ExecutionSummary::default();

    for idx in graph.execution_order() {
        let dependency_signal = graph.dependencies(idx).iter().fold(
            tasks.get(idx).map_or_else(
                || DependencySignal::blocked("missing task", BlockingOutcome::Blocked),
                |task| prior_dependency_signal(*task, prior),
            ),
            |outcome, &dep_idx| {
                outcome.combine(
                    match signals.get(dep_idx).cloned().flatten().unwrap_or_else(|| {
                        DependencySignal::blocked("missing dependency", BlockingOutcome::Blocked)
                    }) {
                        DependencySignal::Blocked { .. }
                            if !graph.blocks_on_failure(idx, dep_idx) =>
                        {
                            DependencySignal::Satisfied
                        }
                        signal @ (DependencySignal::Satisfied
                        | DependencySignal::Blocked { .. }
                        | DependencySignal::Cancelled) => signal,
                    },
                )
            },
        );

        let Some(task) = tasks.get(idx) else {
            continue;
        };
        let assessment = assessments
            .get(&task.task_id())
            .cloned()
            .unwrap_or_else(TaskAssessment::applicable);
        let (signal, status, outcome) =
            dispatch_task(*task, &assessment, ctx, log, &dependency_signal, false);
        if status == TaskStatus::Failed {
            summary.failed_tasks = summary.failed_tasks.saturating_add(1);
        }

        if let Some(slot) = signals.get_mut(idx) {
            *slot = Some(signal);
        }
        summary.record(task.task_id(), task.selector(), task.name(), outcome);
    }
    summary
}

fn prior_dependency_signal(task: &dyn Task, prior: Option<&ExecutionSummary>) -> DependencySignal {
    let Some(prior) = prior else {
        return DependencySignal::Satisfied;
    };
    let blocking = task
        .dependencies()
        .iter()
        .filter_map(|dependency| {
            prior
                .outcome(dependency)
                .map(|outcome| (dependency, outcome))
        })
        .fold(
            DependencySignal::Satisfied,
            |signal, (dependency, outcome)| {
                signal.combine(match outcome {
                    TaskOutcome::Satisfied => DependencySignal::Satisfied,
                    TaskOutcome::Unmet => DependencySignal::blocked(
                        prior.task_name(dependency),
                        BlockingOutcome::Incomplete,
                    ),
                    TaskOutcome::Failed => DependencySignal::blocked(
                        prior.task_name(dependency),
                        BlockingOutcome::Failed,
                    ),
                    TaskOutcome::Blocked => DependencySignal::blocked(
                        prior.task_name(dependency),
                        BlockingOutcome::Blocked,
                    ),
                    TaskOutcome::Cancelled => DependencySignal::Cancelled,
                })
            },
        );
    task.ordering_dependencies()
        .iter()
        .filter_map(|dependency| prior.outcome(dependency))
        .fold(blocking, |signal, outcome| {
            signal.combine(if outcome == TaskOutcome::Cancelled {
                DependencySignal::Cancelled
            } else {
                DependencySignal::Satisfied
            })
        })
}

#[cfg(test)]
#[path = "tests/scheduler/mod.rs"]
mod tests;
