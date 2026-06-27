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

/// Execute a single task, catching any panic.
///
/// Returns `true` if the task completed without panicking.  On panic the task
/// is recorded as [`TaskStatus::Failed`], any buffered output is flushed, and
/// the caller's senders are left un-sent so dependents receive a
/// [`mpsc::RecvError`] and skip themselves.
fn run_task_guarded(task: &dyn Task, ctx: &Context, log: &Arc<Logger>) -> bool {
    log.notify_task_start(task.name());

    let buf = Arc::new(BufferedLog::new(Arc::clone(log)));
    let task_ctx = ctx.with_log(buf.clone() as Arc<dyn Log>);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tasks::execute(task, &task_ctx);
    }));

    if let Err(payload) = result {
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
        log.record_task_outcome(task.name(), task.domain(), TaskStatus::Failed, Some(&msg));
        buf.flush_and_complete(task.name());
        return false;
    }

    buf.flush_and_complete(task.name());
    true
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
    let mut receivers: Vec<Option<mpsc::Receiver<()>>> = Vec::with_capacity(tasks.len());
    let mut dependency_senders: Vec<Option<mpsc::Sender<()>>> = Vec::with_capacity(tasks.len());
    let mut senders: Vec<Vec<mpsc::Sender<()>>> = vec![Vec::new(); tasks.len()];

    for task_idx in 0..tasks.len() {
        let deps = graph.dependencies(task_idx);
        if deps.is_empty() {
            receivers.push(None);
            dependency_senders.push(None);
        } else {
            let (tx, rx) = mpsc::channel::<()>();
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

    // Drop the original senders so a failed dependency closes dependent
    // receivers instead of leaving them blocked forever.
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
                .filter_map(|&dep_idx| tasks.get(dep_idx).map(|task| task.name()))
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

                // Wait for all deps: receive one message per dependency.
                // If recv() returns Err(RecvError) a sender was dropped without
                // sending — the dependency did not complete (e.g. it panicked).
                let deps_ok = rx.is_none_or(|rx| (0..dep_count).all(|_| rx.recv().is_ok()));

                if !deps_ok {
                    let reason = "dependency did not complete";
                    log.diag_task(
                        DiagEvent::TaskSkip,
                        task.name(),
                        &format!("skipped: {reason}"),
                    );
                    log.record_task_outcome(
                        task.name(),
                        task.domain(),
                        TaskStatus::Skipped,
                        Some(reason),
                    );
                    // my_senders is dropped here without sending, propagating
                    // RecvError to any tasks that depend on this one.
                    return;
                }

                if run_task_guarded(task, ctx, log) {
                    // Signal all dependent tasks.
                    for tx in my_senders {
                        if tx.send(()).is_err() {
                            tracing::debug!(
                                "dependent task channel closed before {} signalled completion",
                                task.name()
                            );
                        }
                    }
                }
                // On panic run_task_guarded returns false; my_senders drops
                // here without sending, propagating RecvError to dependents.
            });
        }
    });
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
