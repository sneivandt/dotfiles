//! Dependency-driven parallel task scheduling.
//!
//! Provides [`run_tasks_parallel`](crate::engine::scheduler::run_tasks_parallel) for executing tasks concurrently using OS
//! threads.

use std::sync::{Arc, mpsc};

use super::graph::ResolvedTaskGraph;
use crate::engine::{self, Context, Task};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{self, BufferedLog, Log, LogEvent, Logger, Output as _, TaskStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencySignal {
    Satisfied,
    Blocked,
    Cancelled,
}

impl DependencySignal {
    const fn from_status(status: TaskStatus) -> Self {
        if matches!(status, TaskStatus::Failed) {
            Self::Blocked
        } else {
            Self::Satisfied
        }
    }

    const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Blocked, _) | (_, Self::Blocked) => Self::Blocked,
            (Self::Cancelled, _) | (_, Self::Cancelled) => Self::Cancelled,
            _ => Self::Satisfied,
        }
    }
}

#[derive(Debug)]
struct TaskRuntime {
    dependency_receiver: Option<mpsc::Receiver<DependencySignal>>,
    dependency_sender: Option<mpsc::Sender<DependencySignal>>,
    dependent_senders: Vec<mpsc::Sender<DependencySignal>>,
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
    senders: Vec<mpsc::Sender<DependencySignal>>,
    signal: DependencySignal,
) {
    for tx in senders {
        if tx.send(signal).is_err() {
            tracing::debug!(
                "dependent task channel closed before {task_name} signalled completion"
            );
        }
    }
}

fn record_scheduler_skip(task: &dyn Task, log: &dyn Log, reason: &str) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    log.run_task_event(LogEvent::TaskSkip, task.name(), reason);
    log.debug(reason);
    log.record_task(task.name(), TaskStatus::Skipped, Some(reason));
}

fn dependency_outcome(
    receiver: Option<mpsc::Receiver<DependencySignal>>,
    dependency_count: usize,
) -> DependencySignal {
    let Some(receiver) = receiver else {
        return DependencySignal::Satisfied;
    };

    (0..dependency_count).fold(DependencySignal::Satisfied, |outcome, _| {
        let signal = receiver.recv().unwrap_or(DependencySignal::Blocked);
        outcome.combine(signal)
    })
}

/// Execute a single task, catching any panic.
///
/// Returns the recorded task status. On panic the task is recorded as
/// [`TaskStatus::Failed`], any buffered output is flushed, and dependents are
/// blocked the same way they are for ordinary task failures.
fn run_task_buffered(
    task: &dyn Task,
    ctx: &Context,
    log: &Arc<Logger>,
    notify_start: bool,
) -> TaskStatus {
    if notify_start {
        log.notify_task_start(task.name());
    }
    let buf = Arc::new(BufferedLog::new(Arc::clone(log)));
    let buffered_log: Arc<dyn Log> = Arc::<BufferedLog>::clone(&buf);
    let task_ctx = ctx.with_log(buffered_log);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        engine::execute(task, &task_ctx)
    }));

    let status = match result {
        Ok(status) => status,
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
            log.record_task(task.name(), TaskStatus::Failed, Some(&msg));
            TaskStatus::Failed
        }
    };

    buf.flush_and_complete(task.name(), status);
    status
}

/// Resolve one task against its dependency outcome, running it only when the
/// dependencies were satisfied and the run has not been cancelled.
///
/// Shared by both schedulers so the parallel and sequential paths cannot drift:
/// they differ only in whether a task start notification is emitted, which the
/// sequential path does not need because it never interleaves output.
fn dispatch_task(
    task: &dyn Task,
    ctx: &Context,
    log: &Arc<Logger>,
    dependencies: DependencySignal,
    notify_start: bool,
) -> DependencySignal {
    let skip_reason = match dependencies {
        DependencySignal::Blocked => Some(("dependency failed", DependencySignal::Blocked)),
        DependencySignal::Cancelled => Some(("cancelled", DependencySignal::Cancelled)),
        DependencySignal::Satisfied if ctx.is_cancelled() => {
            Some(("cancelled", DependencySignal::Cancelled))
        }
        DependencySignal::Satisfied => None,
    };

    let Some((reason, signal)) = skip_reason else {
        return DependencySignal::from_status(run_task_buffered(task, ctx, log, notify_start));
    };

    record_scheduler_skip(task, &**log, reason);
    log.emit_task_result_and_redraw(task.name());
    signal
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
pub(crate) fn run_tasks_parallel(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    ctx: &Context,
    log: &Arc<Logger>,
) {
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
                runtime.dependent_senders.push(tx);
            }
        }
    }

    // Drop the original senders so an unexpected panic before signalling closes
    // dependent receivers instead of leaving them blocked forever.
    for runtime in &mut runtimes {
        runtime.dependency_sender = None;
    }

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
                let dependency_signal = dependency_outcome(dependency_receiver, dep_count);
                let signal = dispatch_task(task, ctx, log, dependency_signal, true);

                signal_dependents(task.name(), dependent_senders, signal);
            });
        }
    });
}

/// Run tasks sequentially in dependency-safe order.
///
/// Normal task failures block dependent tasks just like the parallel scheduler;
/// deliberate skips and not-applicable outcomes still satisfy dependencies.
pub(crate) fn run_tasks_sequential(
    tasks: &[&dyn Task],
    graph: &ResolvedTaskGraph,
    ctx: &Context,
    log: &Arc<Logger>,
) {
    let mut signals: Vec<Option<DependencySignal>> = vec![None; tasks.len()];

    for idx in graph.execution_order() {
        let dependency_signal = graph.dependencies(idx).iter().fold(
            DependencySignal::Satisfied,
            |outcome, &dep_idx| {
                outcome.combine(
                    signals
                        .get(dep_idx)
                        .copied()
                        .flatten()
                        .unwrap_or(DependencySignal::Blocked),
                )
            },
        );

        let Some(task) = tasks.get(idx) else {
            continue;
        };
        let signal = dispatch_task(*task, ctx, log, dependency_signal, false);

        if let Some(slot) = signals.get_mut(idx) {
            *slot = Some(signal);
        }
    }
}

#[cfg(test)]
#[path = "tests/scheduler/mod.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
