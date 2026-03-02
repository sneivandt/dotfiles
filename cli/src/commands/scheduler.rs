//! Dependency-driven parallel task scheduling.
//!
//! Provides [`run_tasks_parallel`] for executing tasks concurrently using OS
//! threads.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};

use crate::logging::{self, BufferedLog, DiagEvent, Log, Logger};
use crate::tasks::{self, Context, Task};

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
