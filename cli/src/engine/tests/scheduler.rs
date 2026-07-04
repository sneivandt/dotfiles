use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use super::*;
use crate::tasks::test_helpers::{ContextBuilder, empty_config, make_static_context};
use crate::tasks::{TaskResult, task_deps};

fn make_test_log_and_ctx() -> (Arc<Logger>, Context, logging::TestDispatchLock) {
    let dispatch_lock = logging::test_dispatch_lock();
    let (ctx, log) = make_static_context(empty_config(PathBuf::from("/tmp")));
    (log, ctx, dispatch_lock)
}

fn run_test_tasks(tasks: &[&dyn Task], ctx: &Context, log: &Arc<Logger>) {
    let graph = ResolvedTaskGraph::resolve(tasks).unwrap();
    run_tasks_parallel(tasks, &graph, ctx, log);
}

macro_rules! flag_task {
    ($type_name:ident, $task_name:literal $(, deps: [$($dep:ty),+ $(,)?])?) => {
        struct $type_name {
            ran: Arc<AtomicBool>,
        }

        impl Task for $type_name {
            fn name(&self) -> &'static str {
                $task_name
            }

            fn phase(&self) -> TaskPhase {
                TaskPhase::Provision
            }

            $(task_deps![$($dep),+];)?

            fn run(&self, _ctx: &Context) -> Result<TaskResult> {
                self.ran.store(true, Ordering::SeqCst);
                Ok(TaskResult::Ok)
            }
        }
    };
}

flag_task!(FlagTask, "flag-task");

// -----------------------------------------------------------------------
// Panic task: panics unconditionally, simulating a failed dependency.
// -----------------------------------------------------------------------
struct PanicTask;

impl Task for PanicTask {
    fn name(&self) -> &'static str {
        "panic-task"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    #[allow(clippy::panic, reason = "panicking allowed at this trust boundary")]
    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        panic!("simulated failure");
    }
}

flag_task!(DepOnPanicTask, "dep-on-panic", deps: [PanicTask]);

// -----------------------------------------------------------------------
// Failed task: returns TaskResult::Failed without panicking.
// -----------------------------------------------------------------------
struct FailedTask;

impl Task for FailedTask {
    fn name(&self) -> &'static str {
        "failed-task"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Failed("simulated failure".to_string()))
    }
}

flag_task!(DepOnFailedTask, "dep-on-failed", deps: [FailedTask]);

// -----------------------------------------------------------------------
// Skipped task: returns TaskResult::Skipped, which is non-blocking.
// -----------------------------------------------------------------------
struct SkippedTask;

impl Task for SkippedTask {
    fn name(&self) -> &'static str {
        "skipped-task"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Skipped("deliberate skip".to_string()))
    }
}

flag_task!(DepOnSkippedTask, "dep-on-skipped", deps: [SkippedTask]);

// -----------------------------------------------------------------------
// Chain tasks: PanicTask → ChainB → ChainC.
// -----------------------------------------------------------------------
flag_task!(ChainB, "chain-b", deps: [PanicTask]);
flag_task!(ChainC, "chain-c", deps: [ChainB]);

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

flag_task!(FlagTask2, "flag-task-2");
flag_task!(DepOnFlagTask, "dep-on-flag", deps: [FlagTask]);

// -----------------------------------------------------------------------
// Diamond tasks: A → D, B → D (two independent parents, one child).
// -----------------------------------------------------------------------
flag_task!(DiamondA, "diamond-a");
flag_task!(DiamondB, "diamond-b");
flag_task!(DiamondD, "diamond-d", deps: [DiamondA, DiamondB]);

// -----------------------------------------------------------------------
// Task with a dependency on a type not in the task list.
// -----------------------------------------------------------------------
flag_task!(DepOnMissing, "dep-on-missing", deps: [PanicTask]);

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn independent_task_runs_normally() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let task = FlagTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&task], &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "independent task should have run"
    );
}

#[test]
fn dependent_task_is_skipped_when_dependency_panics() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let panic_task = PanicTask;
    let dep_task = DepOnPanicTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&panic_task, &dep_task], &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "panic-task" && e.status == TaskStatus::Failed),
        "panicked task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-panic" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn dependent_task_is_skipped_when_dependency_fails() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let failed_task = FailedTask;
    let dep_task = DepOnFailedTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&failed_task, &dep_task], &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "failed-task" && e.status == TaskStatus::Failed),
        "failed task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-failed" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn sequential_runner_skips_dependents_when_dependency_fails() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let failed_task = FailedTask;
    let dep_task = DepOnFailedTask {
        ran: Arc::clone(&ran),
    };
    let tasks: Vec<&dyn Task> = vec![&failed_task, &dep_task];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();

    run_tasks_sequential(&tasks, &graph, &ctx);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "failed-task" && e.status == TaskStatus::Failed),
        "failed task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-failed" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn skipped_dependency_satisfies_dependent_task() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let skipped_task = SkippedTask;
    let dep_task = DepOnSkippedTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&skipped_task, &dep_task], &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "deliberately skipped dependencies should not block dependents"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "skipped-task" && e.status == TaskStatus::Skipped),
        "dependency should be recorded as Skipped"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-skipped" && e.status == TaskStatus::Ok),
        "dependent task should be recorded as Ok"
    );
}

#[test]
fn failure_propagates_through_dependency_chain() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_b = Arc::new(AtomicBool::new(false));
    let ran_c = Arc::new(AtomicBool::new(false));
    let panic_task = PanicTask;
    let chain_b = ChainB {
        ran: Arc::clone(&ran_b),
    };
    let chain_c = ChainC {
        ran: Arc::clone(&ran_c),
    };

    run_test_tasks(&[&panic_task, &chain_b, &chain_c], &ctx, &log);

    assert!(!ran_b.load(Ordering::SeqCst), "chain-b should not have run");
    assert!(!ran_c.load(Ordering::SeqCst), "chain-c should not have run");
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "panic-task" && e.status == TaskStatus::Failed),
        "panicked task should be recorded as Failed"
    );
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
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_1 = Arc::new(AtomicBool::new(false));
    let ran_2 = Arc::new(AtomicBool::new(false));
    let task_1 = FlagTask {
        ran: Arc::clone(&ran_1),
    };
    let task_2 = FlagTask2 {
        ran: Arc::clone(&ran_2),
    };

    run_test_tasks(&[&task_1, &task_2], &ctx, &log);

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
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_flag = Arc::new(AtomicBool::new(false));
    let ran_dep = Arc::new(AtomicBool::new(false));
    let flag_task = FlagTask {
        ran: Arc::clone(&ran_flag),
    };
    let dep_task = DepOnFlagTask {
        ran: Arc::clone(&ran_dep),
    };

    run_test_tasks(&[&flag_task, &dep_task], &ctx, &log);

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
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
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

    run_test_tasks(&[&task_a, &task_b, &task_d], &ctx, &log);

    assert!(ran_a.load(Ordering::SeqCst), "diamond-a should have run");
    assert!(ran_b.load(Ordering::SeqCst), "diamond-b should have run");
    assert!(
        ran_d.load(Ordering::SeqCst),
        "diamond-d should have run after both parents completed"
    );
}

#[test]
fn empty_task_list_completes_without_panic() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let empty: Vec<&dyn Task> = vec![];
    run_test_tasks(&empty, &ctx, &log);
    // No panic = success
}

#[test]
fn dependency_not_in_list_is_ignored() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let task = DepOnMissing {
        ran: Arc::clone(&ran),
    };

    // PanicTask is not in the task list, so its dep is filtered out.
    // DepOnMissing should run as if it has no dependencies.
    run_test_tasks(&[&task], &ctx, &log);

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

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
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

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
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
    let (log, _tmp, _guard) = logging::isolated_logger();
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
    let (log, _tmp, _guard) = logging::isolated_logger();
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

/// Regression test: stage header must not be lost when a task calls
/// `ctx.debug_fmt()` during `run()`.
///
/// The regression was introduced by `debug_fmt()` using
/// `tracing::enabled!(Level::DEBUG)` as a guard.  That macro creates a
/// HINT-kind callsite that goes through `Subscriber::enabled()`,
/// setting per-layer `FilterState` bits on the calling thread without
/// dispatching an event to clean them up.  With a two-layer subscriber
/// (INFO-level console + DEBUG-level file), the stale bits caused the
/// `Filtered` console layer to silently drop the subsequent
/// `tracing::info!(target: "dotfiles::stage", …)` replay.
///
/// This test uses a lightweight custom `Layer` (rather than
/// `tracing_subscriber::fmt::Layer`) to record which event targets pass
/// through a `LevelFilter::INFO` filter.  Using a custom layer avoids
/// platform-specific differences in `fmt::Layer` formatting/writing
/// while still exercising the `Filtered` machinery that the original bug
/// corrupted.
#[test]
fn stage_header_not_lost_after_debug_fmt_call() {
    use std::sync::{Arc, Mutex};
    use tracing::Subscriber;
    use tracing_subscriber::{
        Layer as TracingLayer, filter::LevelFilter, layer::SubscriberExt as _,
    };

    /// Minimal layer that records the target of every event it receives.
    struct TargetCapture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> TracingLayer<S> for TargetCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _cx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.metadata().target().to_string());
        }
    }

    // Task that calls ctx.debug_fmt() — simulating what apply.rs does
    // for per-resource "ok: <desc>" messages.
    struct DebugFmtTask;
    impl Task for DebugFmtTask {
        fn name(&self) -> &'static str {
            "debug-fmt-task"
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            // This is the call site that the regression broke:
            // debug_fmt previously used tracing::enabled!(DEBUG) which
            // left stale FilterState bits that silently dropped the
            // subsequent stage INFO event replayed by flush_and_complete.
            ctx.debug_fmt(|| "ok: some/resource".to_string());
            ctx.log.info("1 changed, 0 already ok");
            Ok(TaskResult::Ok)
        }
    }

    // Two-layer subscriber: INFO-filtered capture (simulates console) +
    // DEBUG-filtered file (simulates the diagnostic log). This is the
    // topology where the `FilterState` corruption originally manifested.
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);
    let info_layer = TargetCapture(Arc::clone(&captured)).with_filter(LevelFilter::INFO);
    let subscriber = tracing_subscriber::registry().with(info_layer);
    let _inner_guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(subscriber));

    let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
    let buf = Arc::new(BufferedLog::new(Arc::clone(&log)));
    let task_ctx = ctx.with_log(buf.clone() as Arc<dyn Log>);

    log.notify_task_start("debug-fmt-task");
    tasks::execute(&DebugFmtTask, &task_ctx);
    buf.flush_and_complete("debug-fmt-task");

    let targets = captured
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        targets.iter().any(|t| t == "dotfiles::stage"),
        "stage event must reach the INFO-filtered layer after debug_fmt() was called;\nreceived targets:\n{targets:?}"
    );
}

#[test]
fn dependency_ordering_is_respected() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();

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

    run_test_tasks(&[&dep_task, &flag_task], &ctx, &log);

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
