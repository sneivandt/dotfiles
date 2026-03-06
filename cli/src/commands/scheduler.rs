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

    // -----------------------------------------------------------------------
    // A second independent task type (different TypeId from FlagTask).
    // -----------------------------------------------------------------------
    struct FlagTask2 {
        ran: Arc<AtomicBool>,
    }

    impl Task for FlagTask2 {
        fn name(&self) -> &'static str {
            "flag-task-2"
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
    // Task that depends on FlagTask (for happy-path dependency tests).
    // -----------------------------------------------------------------------
    struct DepOnFlagTask {
        ran: Arc<AtomicBool>,
    }

    impl Task for DepOnFlagTask {
        fn name(&self) -> &'static str {
            "dep-on-flag"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        task_deps![FlagTask];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // Diamond tasks: A → D, B → D (two independent parents, one child).
    // -----------------------------------------------------------------------
    struct DiamondA {
        ran: Arc<AtomicBool>,
    }

    impl Task for DiamondA {
        fn name(&self) -> &'static str {
            "diamond-a"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct DiamondB {
        ran: Arc<AtomicBool>,
    }

    impl Task for DiamondB {
        fn name(&self) -> &'static str {
            "diamond-b"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct DiamondD {
        ran: Arc<AtomicBool>,
    }

    impl Task for DiamondD {
        fn name(&self) -> &'static str {
            "diamond-d"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        task_deps![DiamondA, DiamondB];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // Task with a dependency on a type not in the task list.
    // -----------------------------------------------------------------------
    struct DepOnMissing {
        ran: Arc<AtomicBool>,
    }

    impl Task for DepOnMissing {
        fn name(&self) -> &'static str {
            "dep-on-missing"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        // Depends on PanicTask which won't be in the task list.
        task_deps![PanicTask];

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

    #[test]
    fn multiple_independent_tasks_all_run() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran_1 = Arc::new(AtomicBool::new(false));
        let ran_2 = Arc::new(AtomicBool::new(false));
        let task_1 = FlagTask {
            ran: Arc::clone(&ran_1),
        };
        let task_2 = FlagTask2 {
            ran: Arc::clone(&ran_2),
        };

        run_tasks_parallel(&[&task_1, &task_2], &ctx, &log);

        assert!(
            ran_1.load(Ordering::SeqCst),
            "first independent task should have run"
        );
        assert!(
            ran_2.load(Ordering::SeqCst),
            "second independent task should have run"
        );
    }

    #[test]
    fn task_runs_after_dependency_completes() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran_flag = Arc::new(AtomicBool::new(false));
        let ran_dep = Arc::new(AtomicBool::new(false));
        let flag_task = FlagTask {
            ran: Arc::clone(&ran_flag),
        };
        let dep_task = DepOnFlagTask {
            ran: Arc::clone(&ran_dep),
        };

        run_tasks_parallel(&[&flag_task, &dep_task], &ctx, &log);

        assert!(
            ran_flag.load(Ordering::SeqCst),
            "dependency (FlagTask) should have run"
        );
        assert!(
            ran_dep.load(Ordering::SeqCst),
            "dependent task should have run after its dependency"
        );
    }

    #[test]
    fn diamond_dependency_all_tasks_run() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran_a = Arc::new(AtomicBool::new(false));
        let ran_b = Arc::new(AtomicBool::new(false));
        let ran_d = Arc::new(AtomicBool::new(false));
        let task_a = DiamondA {
            ran: Arc::clone(&ran_a),
        };
        let task_b = DiamondB {
            ran: Arc::clone(&ran_b),
        };
        let task_d = DiamondD {
            ran: Arc::clone(&ran_d),
        };

        run_tasks_parallel(&[&task_a, &task_b, &task_d], &ctx, &log);

        assert!(ran_a.load(Ordering::SeqCst), "diamond-a should have run");
        assert!(ran_b.load(Ordering::SeqCst), "diamond-b should have run");
        assert!(
            ran_d.load(Ordering::SeqCst),
            "diamond-d should have run after both parents completed"
        );
    }

    #[test]
    fn empty_task_list_completes_without_panic() {
        let (log, ctx) = make_test_log_and_ctx();
        let empty: Vec<&dyn Task> = vec![];
        run_tasks_parallel(&empty, &ctx, &log);
        // No panic = success
    }

    #[test]
    fn dependency_not_in_list_is_ignored() {
        let (log, ctx) = make_test_log_and_ctx();
        let ran = Arc::new(AtomicBool::new(false));
        let task = DepOnMissing {
            ran: Arc::clone(&ran),
        };

        // PanicTask is not in the task list, so its dep is filtered out.
        // DepOnMissing should run as if it has no dependencies.
        run_tasks_parallel(&[&task], &ctx, &log);

        assert!(
            ran.load(Ordering::SeqCst),
            "task with missing dependency should run (dep filtered out)"
        );
    }

    // -----------------------------------------------------------------------
    // Stage-header regression tests.
    //
    // Tasks that call `ctx.log.info()` inside `run()` — as `process_resources`
    // does via `stats.finish(ctx)` — must have their `==>` stage header
    // buffered by `execute()` and replayed by `flush_and_complete()`.
    //
    // These tests simulate exactly what `run_tasks_parallel` does per task
    // thread, but in the test thread so the `isolated_logger()` file subscriber
    // captures the replayed tracing events.
    // -----------------------------------------------------------------------

    /// Task that logs a stats summary (like `stats.finish(ctx)`) from inside `run()`.
    struct StatsTask;

    impl Task for StatsTask {
        fn name(&self) -> &'static str {
            "stats-task"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            // Simulates `stats.finish(ctx)` called inside process_resources.
            ctx.log.info("0 changed, 37 already ok");
            Ok(TaskResult::Ok)
        }
    }

    /// Task that logs a named stats summary for multi-task regression tests.
    struct NamedStatsTask {
        name: &'static str,
        count: u32,
    }

    impl Task for NamedStatsTask {
        fn name(&self) -> &'static str {
            self.name
        }

        fn should_run(&self, _: &Context) -> bool {
            true
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            ctx.log
                .info(&format!("0 changed, {} already ok", self.count));
            Ok(TaskResult::Ok)
        }
    }

    /// Regression test: stage header must be present in the log when a task
    /// calls `ctx.log.info()` from within `run()` (the `stats.finish` path).
    ///
    /// Before the regression was detected, tasks producing `"0 changed, X
    /// already ok"` output via `process_resources` were missing their `==>`
    /// stage headers in the console output.
    #[test]
    fn stage_header_present_when_info_logged_in_run() {
        let (log, _tmp, _guard) = crate::logging::isolated_logger();
        let log = Arc::new(log);

        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
        let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
        let task_ctx = ctx.with_log(buf.clone() as Arc<dyn Log>);

        // Exactly mirrors what run_tasks_parallel does per task thread.
        log.notify_task_start("stats-task");
        tasks::execute(&StatsTask, &task_ctx);
        buf.flush_and_complete("stats-task");

        let path = log.log_path().expect("log path");
        let contents = std::fs::read_to_string(path).unwrap();

        let stage_pos = contents
            .find("==> stats-task")
            .expect("stage header must appear in log for task that calls ctx.log.info in run()");
        let info_pos = contents
            .find("0 changed, 37 already ok")
            .expect("stats info must appear in log");

        assert!(
            stage_pos < info_pos,
            "stage header must come before stats info; log:\n{contents}"
        );
    }

    /// Regression test: stage header must be present for multiple parallel tasks
    /// that all produce stats output.  Simulates the scenario where 6 dependent
    /// tasks start after `ReloadConfig` and all complete with `"0 changed, X
    /// already ok"` output, none of which should be missing their `==>` header.
    #[test]
    fn stage_headers_present_for_multiple_concurrent_stats_tasks() {
        let (log, _tmp, _guard) = crate::logging::isolated_logger();
        let log = Arc::new(log);

        let tasks_to_run: &[(&str, u32)] = &[
            ("install-symlinks", 37),
            ("apply-permissions", 3),
            ("configure-systemd", 2),
            ("install-hooks", 1),
        ];

        // Run each task through the same per-thread flow used by run_tasks_parallel.
        for (name, count) in tasks_to_run {
            let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
            let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
            let task_named = NamedStatsTask {
                name,
                count: *count,
            };
            let task_ctx = ctx.with_log(buf.clone() as Arc<dyn Log>);

            log.notify_task_start(name);
            tasks::execute(&task_named, &task_ctx);
            buf.flush_and_complete(name);
        }

        let path = log.log_path().expect("log path");
        let contents = std::fs::read_to_string(path).unwrap();

        for (name, count) in tasks_to_run {
            assert!(
                contents.contains(&format!("==> {name}")),
                "stage header must appear for task '{name}'; log:\n{contents}"
            );
            assert!(
                contents.contains(&format!("0 changed, {count} already ok")),
                "stats info must appear for task '{name}'; log:\n{contents}"
            );
        }
    }

    #[test]
    fn dependency_ordering_is_respected() {
        let (log, ctx) = make_test_log_and_ctx();

        // Use the existing FlagTask → DepOnFlagTask relationship:
        // FlagTask must run before DepOnFlagTask. Verify using order of
        // task completion recorded in the logger.
        let flag_ran = Arc::new(AtomicBool::new(false));
        let dep_ran = Arc::new(AtomicBool::new(false));
        let flag_task = FlagTask {
            ran: Arc::clone(&flag_ran),
        };
        let dep_task = DepOnFlagTask {
            ran: Arc::clone(&dep_ran),
        };

        run_tasks_parallel(&[&dep_task, &flag_task], &ctx, &log);

        // Both must run.
        assert!(flag_ran.load(Ordering::SeqCst), "flag-task should have run");
        assert!(
            dep_ran.load(Ordering::SeqCst),
            "dep-on-flag should have run"
        );

        // dep-on-flag depends on FlagTask, so FlagTask must complete first.
        // The logger records tasks in completion order.
        let entries = log.task_entries();
        let flag_pos = entries.iter().position(|e| e.name == "flag-task");
        let dep_pos = entries.iter().position(|e| e.name == "dep-on-flag");
        assert!(
            flag_pos.is_some() && dep_pos.is_some(),
            "both tasks should be recorded in the logger"
        );
        assert!(
            flag_pos.unwrap() < dep_pos.unwrap(),
            "flag-task should complete before dep-on-flag (dependency ordering)"
        );
    }
}
