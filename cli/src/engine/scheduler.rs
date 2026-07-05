//! Dependency-driven parallel task scheduling.
//!
//! Provides [`run_tasks_parallel`](crate::engine::scheduler::run_tasks_parallel) for executing tasks concurrently using OS
//! threads.

use std::sync::{Arc, mpsc};

use super::graph::ResolvedTaskGraph;
use crate::logging::{self, BufferedLog, DiagEvent, Log, Logger, Output as _, TaskStatus};
#[cfg(test)]
use crate::tasks::TaskPhase;
use crate::tasks::{self, Context, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencySignal {
    Satisfied,
    Blocked,
}

impl DependencySignal {
    const fn from_status(status: TaskStatus) -> Self {
        if matches!(status, TaskStatus::Failed) {
            Self::Blocked
        } else {
            Self::Satisfied
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

fn record_dependency_block(task: &dyn Task, log: &dyn Log) {
    let reason = "dependency failed";
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    log.diag_task(
        DiagEvent::TaskSkip,
        task.name(),
        &format!("skipped: {reason}"),
    );
    log.info(&format!("skipped: {reason}"));
    log.record_task_outcome(
        task.name(),
        task.domain(),
        TaskStatus::Skipped,
        Some(reason),
    );
}

/// Execute a single task, catching any panic.
///
/// Returns the recorded task status. On panic the task is recorded as
/// [`TaskStatus::Failed`], any buffered output is flushed, and dependents are
/// blocked the same way they are for ordinary task failures.
fn run_task_guarded(task: &dyn Task, ctx: &Context, log: &Arc<Logger>) -> TaskStatus {
    run_task_buffered(task, ctx, log, true)
}

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
        tasks::execute(task, &task_ctx)
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
            log.diag_task(DiagEvent::TaskFail, task.name(), &msg);
            buf.error(&format!("{}: {msg}", task.name()));
            log.record_task_outcome(task.name(), task.domain(), TaskStatus::Failed, Some(&msg));
            TaskStatus::Failed
        }
    };

    buf.flush_and_complete(task.name(), status);
    status
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
    // For each task, create a channel sized to its dep count.
    // senders[i] accumulates all Senders that task i must signal when it completes.
    let mut receivers: Vec<Option<mpsc::Receiver<DependencySignal>>> =
        Vec::with_capacity(tasks.len());
    let mut dependency_senders: Vec<Option<mpsc::Sender<DependencySignal>>> =
        Vec::with_capacity(tasks.len());
    let mut senders: Vec<Vec<mpsc::Sender<DependencySignal>>> = vec![Vec::new(); tasks.len()];

    for task_idx in 0..tasks.len() {
        let deps = graph.dependencies(task_idx);
        if deps.is_empty() {
            receivers.push(None);
            dependency_senders.push(None);
        } else {
            let (tx, rx) = mpsc::channel::<DependencySignal>();
            receivers.push(Some(rx));
            dependency_senders.push(Some(tx));
        }
    }

    for dep_idx in 0..tasks.len() {
        for &dependent_idx in graph.dependents(dep_idx) {
            if let Some(tx) = dependency_senders
                .get(dependent_idx)
                .and_then(Option::as_ref)
                && let Some(s) = senders.get_mut(dep_idx)
            {
                s.push(tx.clone());
            }
        }
    }

    // Drop the original senders so an unexpected panic before signalling closes
    // dependent receivers instead of leaving them blocked forever.
    drop(dependency_senders);

    std::thread::scope(|s| {
        for (idx, ((task, rx), my_senders)) in tasks
            .iter()
            .zip(receivers.iter_mut())
            .zip(senders.iter_mut())
            .enumerate()
        {
            let task = *task;
            let rx = rx.take();
            let my_senders = std::mem::take(my_senders);
            let dep_names: Vec<&str> = graph
                .dependencies(idx)
                .iter()
                .filter_map(|&dep_idx| tasks.get(dep_idx).map(|dep_task| dep_task.name()))
                .collect();
            let dep_count = dep_names.len();

            s.spawn(move || {
                logging::set_diag_thread_name(task.name());

                if let Some(diag) = log.diagnostic() {
                    if dep_names.is_empty() {
                        diag.emit_task(DiagEvent::TaskWait, task.name(), "no deps, ready");
                    } else {
                        diag.emit_task(
                            DiagEvent::TaskWait,
                            task.name(),
                            &format!("waiting for: {}", dep_names.join(", ")),
                        );
                    }
                }

                // Wait for all deps: receive one outcome per dependency.
                // A normal task failure sends Blocked; RecvError is retained as
                // a defensive guard for panics before dependency signalling.
                let deps_ok = rx.is_none_or(|rx| {
                    (0..dep_count).all(|_| matches!(rx.recv(), Ok(DependencySignal::Satisfied)))
                });

                if !deps_ok {
                    record_dependency_block(task, &**log);
                    signal_dependents(task.name(), my_senders, DependencySignal::Blocked);
                    return;
                }

                let status = run_task_guarded(task, ctx, log);
                signal_dependents(
                    task.name(),
                    my_senders,
                    DependencySignal::from_status(status),
                );
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
        if ctx.is_cancelled() {
            ctx.log.warn("cancelled - stopping before next task");
            break;
        }

        let deps_ok = graph.dependencies(idx).iter().all(|&dep_idx| {
            matches!(
                signals.get(dep_idx).copied().flatten(),
                Some(DependencySignal::Satisfied)
            )
        });

        let signal = if deps_ok {
            let Some(task) = tasks.get(idx) else {
                continue;
            };
            let status = run_task_buffered(*task, ctx, log, false);
            DependencySignal::from_status(status)
        } else {
            if let Some(task) = tasks.get(idx) {
                record_dependency_block(*task, &*ctx.log);
            }
            DependencySignal::Blocked
        };

        if let Some(slot) = signals.get_mut(idx) {
            *slot = Some(signal);
        }
    }
}

#[cfg(test)]
#[path = "tests/scheduler.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
