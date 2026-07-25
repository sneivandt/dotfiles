//! Scheduler tests, split by the behaviour under test.
//!
//! Shared task doubles and fixtures live here; each submodule owns one concern
//! so a failure names the behaviour rather than one large file.

use std::path::PathBuf;

use std::sync::Arc;

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use super::*;

use crate::engine::{TaskResult, TaskStats, execute, task_deps};

use crate::infra::logging::{MsgKind, Output, TaskRecorder};

use crate::test_helpers::{ContextBuilder, empty_config, make_static_context};

mod cancellation;
mod dependencies;
mod sequential;
mod stage_headers;

fn make_test_log_and_ctx() -> (Arc<Logger>, Context, logging::TestDispatchLock) {
    let dispatch_lock = logging::test_dispatch_lock();
    let (ctx, log) = make_static_context(empty_config(PathBuf::from("/tmp")));
    (log, ctx, dispatch_lock)
}

fn run_test_tasks(tasks: &[&dyn Task], ctx: &Context, log: &Arc<Logger>) {
    let graph = ResolvedTaskGraph::resolve(tasks).unwrap();
    run_tasks_parallel(tasks, &graph, ctx, log);
}

fn run_test_tasks_with_mode(tasks: &[&dyn Task], ctx: &Context, log: &Arc<Logger>, parallel: bool) {
    let graph = ResolvedTaskGraph::resolve(tasks).unwrap();
    if parallel {
        run_tasks_parallel(tasks, &graph, ctx, log);
    } else {
        run_tasks_sequential(tasks, &graph, ctx, log);
    }
}

fn buffered_log_arc(buf: &Arc<BufferedLog>) -> Arc<dyn Log> {
    Arc::<BufferedLog>::clone(buf)
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

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Failed("simulated failure".to_string()))
    }
}

struct CancellingTask;

impl Task for CancellingTask {
    fn name(&self) -> &'static str {
        "cancelling"
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        ctx.cancellation_token().cancel();
        Ok(TaskResult::Ok)
    }
}

struct CancelAfterFailureTask {
    log: Arc<Logger>,
}

impl Task for CancelAfterFailureTask {
    fn name(&self) -> &'static str {
        "cancel-after-failure"
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        for _ in 0..10_000 {
            if self
                .log
                .task_entries()
                .iter()
                .any(|entry| entry.status == TaskStatus::Failed)
            {
                ctx.cancellation_token().cancel();
                return Ok(TaskResult::Ok);
            }
            std::thread::yield_now();
        }
        anyhow::bail!("timed out waiting for failed task recording")
    }
}

flag_task!(DepOnFailedTask, "dep-on-failed", deps: [FailedTask]);

flag_task!(DepOnCancellingTask, "dep-on-cancelling", deps: [CancellingTask]);

flag_task!(
    DepOnFailureAndCancelTask,
    "dep-on-failure-and-cancel",
    deps: [FailedTask, CancelAfterFailureTask]
);

// -----------------------------------------------------------------------
// Skipped task: returns TaskResult::Skipped, which is non-blocking.
// -----------------------------------------------------------------------
struct SkippedTask;

impl Task for SkippedTask {
    fn name(&self) -> &'static str {
        "skipped-task"
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

struct SequentialChangedDetailTask;

impl Task for SequentialChangedDetailTask {
    fn name(&self) -> &'static str {
        "sequential-detail-task"
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        ctx.log().info("installed: demo-package");
        Ok(TaskStats::changed_with_message("1 changed, 0 already ok").finish())
    }
}

// -----------------------------------------------------------------------
// Stage-header regression tests.
//
// Tasks whose structured result causes `execute()` to log a summary must have
// their `==>` stage header replayed by `flush_and_complete()`.
//
// These tests simulate exactly what `run_tasks_parallel` does per task
// thread, but in the test thread so the `isolated_logger()` file subscriber
// captures the replayed tracing events.
// -----------------------------------------------------------------------

/// Task that returns structured stats for central reporting.
struct StatsTask;

impl Task for StatsTask {
    fn name(&self) -> &'static str {
        "stats-task"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskStats {
            already_ok: 37,
            ..TaskStats::default()
        }
        .finish())
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

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskStats {
            already_ok: self.count,
            ..TaskStats::default()
        }
        .finish())
    }
}
