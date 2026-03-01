//! Dependency-driven parallel task scheduling.
//!
//! Provides [`has_cycle`] for detecting cycles in the dependency graph and
//! [`run_tasks_parallel`] for executing tasks concurrently using OS threads.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};

use crate::logging::{self, BufferedLog, DiagEvent, Log, Logger};
use crate::tasks::{self, Context, Task};

/// Detect cycles in the task dependency graph using Kahn's algorithm.
///
/// Returns `true` if the graph contains at least one cycle.
pub(super) fn has_cycle(tasks: &[&dyn Task]) -> bool {
    let type_to_idx: HashMap<TypeId, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.task_id(), i))
        .collect();

    let mut in_degree: Vec<usize> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| type_to_idx.contains_key(d))
                .count()
        })
        .collect();

    let mut reverse_deps: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    for (i, t) in tasks.iter().enumerate() {
        for dep in t.dependencies() {
            if let Some(&dep_idx) = type_to_idx.get(dep)
                && let Some(rd) = reverse_deps.get_mut(dep_idx)
            {
                rd.push(i);
            }
        }
    }

    let mut queue: Vec<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let mut processed = 0usize;

    while let Some(idx) = queue.pop() {
        processed += 1;
        if let Some(dependents) = reverse_deps.get(idx) {
            for &dep in dependents {
                if let Some(count) = in_degree.get_mut(dep) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push(dep);
                    }
                }
            }
        }
    }

    processed != tasks.len()
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

    // Build TypeId → name and TypeId → index maps for diagnostics and channel wiring.
    let id_to_name: HashMap<TypeId, &str> = tasks.iter().map(|t| (t.task_id(), t.name())).collect();
    let id_to_idx: HashMap<TypeId, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.task_id(), i))
        .collect();

    // For each task, create a channel sized to its dep count.
    // senders[i] accumulates all Senders that task i must signal when it completes.
    let mut receivers: Vec<Option<mpsc::Receiver<()>>> = Vec::with_capacity(tasks.len());
    let mut senders: Vec<Vec<mpsc::Sender<()>>> = vec![Vec::new(); tasks.len()];

    for deps in &resolved_deps {
        if deps.is_empty() {
            receivers.push(None);
        } else {
            let (tx, rx) = mpsc::channel::<()>();
            receivers.push(Some(rx));
            for dep_id in deps {
                if let Some(&dep_idx) = id_to_idx.get(dep_id)
                    && let Some(s) = senders.get_mut(dep_idx)
                {
                    s.push(tx.clone());
                }
            }
        }
    }

    std::thread::scope(|s| {
        for (((task, rx), my_senders), deps) in tasks
            .iter()
            .zip(receivers.iter_mut())
            .zip(senders.iter_mut())
            .zip(resolved_deps.iter())
        {
            let task = *task;
            let rx = rx.take();
            let my_senders = std::mem::take(my_senders);
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

                // Wait for all deps: receive one message per dependency.
                if let Some(rx) = rx {
                    for _ in 0..deps.len() {
                        let _ = rx.recv();
                    }
                }

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

                // Signal all dependent tasks.
                for tx in my_senders {
                    let _ = tx.send(());
                }
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
    mock_task!(TaskC, "c", &[]);

    // Chain: DepA → DepB → DepC
    mock_task!(DepA, "dep-a", &[]);
    mock_task!(DepB, "dep-b", &[TypeId::of::<DepA>()]);
    mock_task!(DepC, "dep-c", &[TypeId::of::<DepB>()]);

    // Diamond: DiaA → DiaB + DiaC → DiaD
    mock_task!(DiaA, "dia-a", &[]);
    mock_task!(DiaB, "dia-b", &[TypeId::of::<DiaA>()]);
    mock_task!(DiaC, "dia-c", &[TypeId::of::<DiaA>()]);
    mock_task!(DiaD, "dia-d", &[TypeId::of::<DiaB>(), TypeId::of::<DiaC>()]);

    // Cyclic: CycA → CycB → CycA
    mock_task!(CycA, "cyc-a", &[TypeId::of::<CycB>()]);
    mock_task!(CycB, "cyc-b", &[TypeId::of::<CycA>()]);

    // Missing dep
    struct MissingDepTask;
    impl Task for MissingDepTask {
        fn name(&self) -> &'static str {
            "missing-dep"
        }
        fn dependencies(&self) -> &[TypeId] {
            // Points to a TypeId that won't be present in the task list
            const DEPS: &[TypeId] = &[TypeId::of::<DepC>()];
            DEPS
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // has_cycle
    // -----------------------------------------------------------------------

    #[test]
    fn no_cycle_independent_tasks() {
        let tasks: Vec<&dyn Task> = vec![&TaskA, &TaskB, &TaskC];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn no_cycle_linear_chain() {
        let tasks: Vec<&dyn Task> = vec![&DepA, &DepB, &DepC];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn no_cycle_diamond() {
        let tasks: Vec<&dyn Task> = vec![&DiaA, &DiaB, &DiaC, &DiaD];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn cycle_detected() {
        let tasks: Vec<&dyn Task> = vec![&CycA, &CycB];
        assert!(has_cycle(&tasks));
    }

    #[test]
    fn missing_dep_not_a_cycle() {
        let tasks: Vec<&dyn Task> = vec![&MissingDepTask, &TaskA];
        assert!(!has_cycle(&tasks));
    }

    // -----------------------------------------------------------------------
    // install order: verify real tasks form a valid DAG
    // -----------------------------------------------------------------------

    #[test]
    fn install_tasks_have_resolvable_dependencies() {
        let tasks = crate::tasks::all_install_tasks();
        let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
        for task in &tasks {
            for dep in task.dependencies() {
                assert!(
                    present.contains(dep),
                    "task '{}' depends on a TypeId not in the task list",
                    task.name()
                );
            }
        }
    }

    #[test]
    fn install_tasks_have_no_cycles() {
        let tasks = crate::tasks::all_install_tasks();
        let task_refs: Vec<&dyn Task> = tasks.iter().map(Box::as_ref).collect();
        assert!(
            !has_cycle(&task_refs),
            "install task graph should not contain cycles"
        );
    }
}
