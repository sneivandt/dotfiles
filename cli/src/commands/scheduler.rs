//! Dependency-driven parallel task scheduling.
//!
//! Provides [`run_tasks_parallel`] for executing tasks concurrently using OS
//! threads.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};

use crate::logging::{self, BufferedLog, DiagEvent, Log, Logger, TaskStatus};
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
                // If recv() returns Err(RecvError) a sender was dropped without
                // sending — the dependency did not complete (e.g. it panicked).
                let deps_ok = rx.is_none_or(|rx| (0..deps.len()).all(|_| rx.recv().is_ok()));

                if !deps_ok {
                    if let Some(diag) = log.diagnostic() {
                        diag.emit_task(
                            DiagEvent::TaskSkip,
                            task.name(),
                            "skipped: dependency did not complete",
                        );
                    }
                    log.record_task(
                        task.name(),
                        TaskStatus::Skipped,
                        Some("dependency did not complete"),
                    );
                    // my_senders is dropped here without sending, propagating
                    // RecvError to any tasks that depend on this one.
                    return;
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
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use anyhow::Result;

    use super::*;
    use crate::tasks::test_helpers::{ContextBuilder, empty_config};
    use crate::tasks::{TaskResult, task_deps};

    fn make_test_log_and_ctx() -> (Arc<Logger>, Context) {
        let log = Arc::new(Logger::new("test"));
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
        (log, ctx)
    }

    // -----------------------------------------------------------------------
    // Independent task: sets a flag when it runs.
    // -----------------------------------------------------------------------
    struct FlagTask {
        ran: Arc<AtomicBool>,
    }

    impl Task for FlagTask {
        fn name(&self) -> &'static str {
            "flag-task"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // Panic task: panics unconditionally, simulating a failed dependency.
    // -----------------------------------------------------------------------
    struct PanicTask;

    impl Task for PanicTask {
        fn name(&self) -> &'static str {
            "panic-task"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        #[allow(clippy::panic)]
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            panic!("simulated failure");
        }
    }

    // -----------------------------------------------------------------------
    // Task that depends on PanicTask; sets a flag if it runs.
    // -----------------------------------------------------------------------
    struct DepOnPanicTask {
        ran: Arc<AtomicBool>,
    }

    impl Task for DepOnPanicTask {
        fn name(&self) -> &'static str {
            "dep-on-panic"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        task_deps![PanicTask];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // Chain tasks: PanicTask → ChainB → ChainC.
    // -----------------------------------------------------------------------
    struct ChainB {
        ran: Arc<AtomicBool>,
    }

    impl Task for ChainB {
        fn name(&self) -> &'static str {
            "chain-b"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        task_deps![PanicTask];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct ChainC {
        ran: Arc<AtomicBool>,
    }

    impl Task for ChainC {
        fn name(&self) -> &'static str {
            "chain-c"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        task_deps![ChainB];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn independent_task_runs_normally() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran = Arc::new(AtomicBool::new(false));
        let task = FlagTask {
            ran: Arc::clone(&ran),
        };

        run_tasks_parallel(&[&task], &ctx, &log);

        assert!(
            ran.load(Ordering::SeqCst),
            "independent task should have run"
        );
    }

    #[test]
    fn dependent_task_is_skipped_when_dependency_panics() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran = Arc::new(AtomicBool::new(false));
        let panic_task = PanicTask;
        let dep_task = DepOnPanicTask {
            ran: Arc::clone(&ran),
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_tasks_parallel(&[&panic_task, &dep_task], &ctx, &log);
        }));

        assert!(result.is_err(), "scheduler should propagate the panic");
        assert!(
            !ran.load(Ordering::SeqCst),
            "dependent task should not have run"
        );
        assert!(
            log.task_entries()
                .iter()
                .any(|e| e.name == "dep-on-panic" && e.status == TaskStatus::Skipped),
            "dependent task should be recorded as Skipped"
        );
    }

    #[test]
    fn failure_propagates_through_dependency_chain() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran_b = Arc::new(AtomicBool::new(false));
        let ran_c = Arc::new(AtomicBool::new(false));
        let panic_task = PanicTask;
        let chain_b = ChainB {
            ran: Arc::clone(&ran_b),
        };
        let chain_c = ChainC {
            ran: Arc::clone(&ran_c),
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_tasks_parallel(&[&panic_task, &chain_b, &chain_c], &ctx, &log);
        }));

        assert!(result.is_err(), "scheduler should propagate the panic");
        assert!(!ran_b.load(Ordering::SeqCst), "chain-b should not have run");
        assert!(!ran_c.load(Ordering::SeqCst), "chain-c should not have run");
        let entries = log.task_entries();
        assert!(
            entries
                .iter()
                .any(|e| e.name == "chain-b" && e.status == TaskStatus::Skipped),
            "chain-b should be recorded as Skipped"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.name == "chain-c" && e.status == TaskStatus::Skipped),
            "chain-c should be recorded as Skipped"
        );
    }
}
