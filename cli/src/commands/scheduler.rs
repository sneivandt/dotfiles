//! Dependency-driven parallel task scheduling.
//!
//! Provides [`TaskGraph`] for tracking task completions and [`run_tasks_parallel`] for
//! executing tasks concurrently using OS threads.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use crate::logging::{self, BufferedLog, DiagEvent, Log, Logger};
use crate::tasks::{self, Context, Task};

/// Shared state for dependency-driven parallel task scheduling.
///
/// Tasks call [`wait_for_deps`](TaskGraph::wait_for_deps) before starting and
/// [`mark_complete`](TaskGraph::mark_complete) when finished.  The [`Condvar`]
/// wakes all waiting tasks whenever a new completion is recorded.
#[derive(Debug, Default)]
struct TaskGraph {
    /// Set of completed task [`TypeId`]s.
    completed: Mutex<HashSet<TypeId>>,
    /// Notified whenever a task completes.
    condvar: Condvar,
}

impl TaskGraph {
    /// Create a new, empty task graph with no completed tasks.
    fn new() -> Self {
        Self::default()
    }

    /// Block until every [`TypeId`] in `deps` has been marked complete.
    fn wait_for_deps(&self, deps: &[TypeId]) {
        if deps.is_empty() {
            return;
        }
        let mut completed = self
            .completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !deps.iter().all(|d| completed.contains(d)) {
            completed = self
                .condvar
                .wait(completed)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(completed);
    }

    /// Record a task as complete and wake all waiting threads.
    fn mark_complete(&self, id: TypeId) {
        let mut completed = self
            .completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        completed.insert(id);
        drop(completed);
        self.condvar.notify_all();
    }
}

/// Run tasks in parallel using a dependency graph.
///
/// Each task is spawned into an OS thread (via `std::thread::scope`) and waits
/// for its dependencies to complete before executing.  OS threads are used
/// deliberately — blocking on a `Condvar` inside a Rayon worker would exhaust
/// Rayon's fixed-size thread pool and deadlock when the pool is smaller than
/// the number of tasks with unsatisfied dependencies (common on 2-vCPU CI
/// runners).  Output is buffered per-task and flushed to the console
/// immediately on completion.
pub(super) fn run_tasks_parallel(tasks: &[&dyn Task], ctx: &Context, log: &Arc<Logger>) {
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    let resolved_deps: Vec<Vec<TypeId>> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| present.contains(d))
                .copied()
                .collect()
        })
        .collect();

    // Build TypeId → name map for diagnostic dep messages.
    let id_to_name: HashMap<TypeId, &str> = tasks.iter().map(|t| (t.task_id(), t.name())).collect();

    let graph = TaskGraph::new();

    std::thread::scope(|s| {
        for (task, deps) in tasks.iter().zip(resolved_deps.iter()) {
            let task = *task;
            let graph = &graph;
            let id_to_name = &id_to_name;
            s.spawn(move || {
                logging::set_diag_thread_name(task.name());

                if let Some(diag) = log.diagnostic() {
                    if deps.is_empty() {
                        diag.emit_task(DiagEvent::TaskWait, task.name(), "no deps, ready");
                    } else {
                        let dep_names: Vec<&str> = deps
                            .iter()
                            .filter_map(|d| id_to_name.get(d).copied())
                            .collect();
                        diag.emit_task(
                            DiagEvent::TaskWait,
                            task.name(),
                            &format!("waiting for: {}", dep_names.join(", ")),
                        );
                    }
                }

                graph.wait_for_deps(deps);

                if let Some(diag) = log.diagnostic() {
                    diag.emit_task(
                        DiagEvent::TaskStart,
                        task.name(),
                        "deps satisfied, executing",
                    );
                }

                log.notify_task_start(task.name());

                let buf = Arc::new(BufferedLog::new(Arc::clone(log)));
                let task_ctx = ctx.with_log(buf.clone() as Arc<dyn Log>);
                tasks::execute(task, &task_ctx);

                if let Some(diag) = log.diagnostic() {
                    diag.emit_task(DiagEvent::TaskDone, task.name(), "");
                }

                buf.flush_and_complete(task.name());
                graph.mark_complete(task.task_id());
            });
        }
    });
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::{Context, TaskResult};

    use anyhow::Result;

    // -----------------------------------------------------------------------
    // Mock tasks — each is a distinct type so TypeId-based deps work.
    // -----------------------------------------------------------------------

    macro_rules! mock_task {
        ($name:ident, $display:expr, $deps:expr) => {
            struct $name;
            impl Task for $name {
                fn name(&self) -> &str {
                    $display
                }
                fn dependencies(&self) -> &[TypeId] {
                    const DEPS: &[TypeId] = $deps;
                    DEPS
                }
                fn should_run(&self, _ctx: &Context) -> bool {
                    true
                }
                fn run(&self, _ctx: &Context) -> Result<TaskResult> {
                    Ok(TaskResult::Ok)
                }
            }
        };
    }

    // Simple tasks for basic tests
    mock_task!(TaskA, "a", &[]);
    mock_task!(TaskB, "b", &[]);

    // -----------------------------------------------------------------------
    // TaskGraph
    // -----------------------------------------------------------------------

    #[test]
    fn graph_no_deps_does_not_block() {
        let graph = TaskGraph::new();
        graph.wait_for_deps(&[]);
    }

    #[test]
    fn graph_satisfied_deps_do_not_block() {
        let graph = TaskGraph::new();
        let id = TypeId::of::<TaskA>();
        graph.mark_complete(id);
        graph.wait_for_deps(&[id]);
    }

    #[test]
    fn graph_notifies_waiters() {
        let graph = std::sync::Arc::new(TaskGraph::new());
        let id = TypeId::of::<TaskA>();
        let g = std::sync::Arc::clone(&graph);
        let handle = std::thread::spawn(move || {
            g.wait_for_deps(&[id]);
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        graph.mark_complete(id);
        handle.join().expect("waiter thread should complete");
    }

    #[test]
    fn graph_multiple_deps_all_required() {
        let graph = std::sync::Arc::new(TaskGraph::new());
        let id_a = TypeId::of::<TaskA>();
        let id_b = TypeId::of::<TaskB>();
        let g = std::sync::Arc::clone(&graph);
        let handle = std::thread::spawn(move || {
            g.wait_for_deps(&[id_a, id_b]);
        });
        graph.mark_complete(id_a);
        // Only one dep satisfied — thread should still be waiting.
        std::thread::sleep(std::time::Duration::from_millis(50));
        graph.mark_complete(id_b);
        handle.join().expect("waiter thread should complete");
    }
}
