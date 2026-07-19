use super::*;
use crate::engine::{
    IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState, TaskStats,
};
use crate::infra::logging::TaskStatus;
use crate::test_helpers::{empty_config, make_static_context};
use anyhow::Result;
use std::cell::Cell;
use std::path::PathBuf;

thread_local! {
    static RESOURCE_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
    static BATCH_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
struct DummyResource;

impl Resource for DummyResource {
    fn description(&self) -> String {
        "dummy".to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        Ok(ResourceChange::AlreadyCorrect)
    }
}

impl IntrinsicState for DummyResource {
    fn current_state(&self) -> Result<ResourceState> {
        Ok(ResourceState::Correct)
    }
}

resource_task! {
    /// Test-only task for resource-task macro behaviour.
    CountingResourceTask {
        name: "Counting resource task",
        items: |_ctx| {
            RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
            Vec::<()>::new()
        },
        build: |_item, _ctx| DummyResource,
        opts: ProcessOpts::strict("count"),
    }
}

resource_task! {
    /// Test-only task for batch-resource-task macro behaviour.
    CountingBatchTask {
        name: "Counting batch task",
        items: |_ctx| {
            BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
            Vec::<()>::new()
        },
        cache: |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
        build: |_item, _ctx| DummyResource,
        state: |_resource, _cache| ResourceState::Correct,
        opts: ProcessOpts::strict("count"),
    }
}

/// A mock task for testing `execute()`.
struct MockTask {
    name: &'static str,
    should_run: bool,
    result: Result<TaskResult, String>,
}

impl Task for MockTask {
    fn name(&self) -> &str {
        self.name
    }
    fn should_run(&self, _ctx: &Context) -> bool {
        self.should_run
    }
    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.result.clone().map_err(|s| anyhow::anyhow!("{s}"))
    }
}

struct ValidationOkTask;

impl Task for ValidationOkTask {
    fn name(&self) -> &'static str {
        "validation-ok"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Validation
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Ok)
    }
}

struct GatedTask {
    ran: std::sync::Arc<std::sync::atomic::AtomicBool>,
    should_run: bool,
    needs_elevation: bool,
}

impl Task for GatedTask {
    fn name(&self) -> &'static str {
        "gated-task"
    }
    fn should_run(&self, _ctx: &Context) -> bool {
        self.should_run
    }
    fn needs_elevation(&self, _ctx: &Context) -> bool {
        self.needs_elevation
    }
    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(TaskResult::Ok)
    }
}

#[test]
fn execute_skips_non_applicable_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "test-task",
        should_run: false,
        result: Ok(TaskResult::Ok),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::NotApplicable);
    assert_eq!(log.failure_count(), 0);
    let entries = log.task_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "test-task");
    assert_eq!(entries[0].status, TaskStatus::NotApplicable);
}

#[test]
fn execute_records_ok_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "ok-task",
        should_run: true,
        result: Ok(TaskResult::Ok),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn execute_records_validation_ok_task_as_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);

    execute(&ValidationOkTask, &ctx);

    let entries = log.task_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, TaskStatus::Changed);
    assert_eq!(entries[0].name, "validation-ok");
}

#[test]
fn execute_records_ok_task_with_message() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "ok-task",
        should_run: true,
        result: Ok(TaskResult::OkWithMessage("created config file".to_string())),
    };

    execute(&task, &ctx);

    let entries = log.task_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, TaskStatus::Changed);
    assert_eq!(entries[0].message.as_deref(), Some("created config file"));
    assert_eq!(entries[0].actions.applied, 1);
}

#[test]
fn execute_records_failed_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "fail-task",
        should_run: true,
        result: Err("kaboom".to_string()),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 1);
}

#[test]
fn execute_records_skipped_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "skip-task",
        should_run: true,
        result: Ok(TaskResult::Skipped("not needed".to_string())),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn execute_records_batch_action_counts() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats {
            changed: 3,
            already_ok: 5,
            skipped: 2,
            failed: 0,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Changed);

    let entry = &log.task_entries()[0];
    assert_eq!(entry.actions.applied, 3);
    assert_eq!(entry.actions.planned, 0);
    assert_eq!(entry.actions.skipped, 2);
    assert_eq!(entry.actions.failed, 0);
}

#[test]
fn execute_records_dry_run_batch_as_planned_actions() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ctx = ctx.with_dry_run(true);
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats {
            changed: 4,
            already_ok: 1,
            skipped: 0,
            failed: 0,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::DryRun);

    let entry = &log.task_entries()[0];
    assert_eq!(entry.actions.applied, 0);
    assert_eq!(entry.actions.planned, 4);
}

#[test]
fn execute_records_failed_batch_and_preserves_action_counts() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats {
            changed: 1,
            already_ok: 0,
            skipped: 2,
            failed: 3,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Failed);

    let entry = &log.task_entries()[0];
    assert_eq!(entry.actions.applied, 1);
    assert_eq!(entry.actions.skipped, 2);
    assert_eq!(entry.actions.failed, 3);
}

#[test]
fn execute_records_skipped_only_batch_as_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats {
            changed: 0,
            already_ok: 2,
            skipped: 3,
            failed: 0,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Skipped);
    assert_eq!(log.task_entries()[0].actions.skipped, 3);
}

#[test]
fn execute_downgrades_cancelled_batch_failure_to_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    ctx.cancellation_token().cancel();
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats {
            changed: 0,
            already_ok: 0,
            skipped: 0,
            failed: 1,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Skipped);
    assert_eq!(log.failure_count(), 0);
    assert_eq!(log.task_entries()[0].actions.failed, 1);
}

#[test]
fn execute_records_task_result_failed_as_failure() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "failed-task",
        should_run: true,
        result: Ok(TaskResult::Failed("git pull failed".to_string())),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 1);
    assert_eq!(log.task_entries()[0].actions.failed, 1);
}

#[test]
fn execute_records_dry_run_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "dry-task",
        should_run: true,
        result: Ok(TaskResult::DryRun),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
    assert_eq!(log.task_entries()[0].actions.planned, 1);
}

#[test]
fn execute_checks_applicability_before_running_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = GatedTask {
        ran: std::sync::Arc::clone(&ran),
        should_run: false,
        needs_elevation: false,
    };

    execute(&task, &ctx);

    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn requires_elevation_respects_prediction_and_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: true,
        needs_elevation: true,
    };

    assert!(task.requires_elevation(&ctx));
    assert!(!task.requires_elevation(&ctx.with_dry_run(true)));
}

#[test]
fn requires_elevation_respects_prediction() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: true,
        needs_elevation: false,
    };

    assert!(!task.requires_elevation(&ctx));
}

#[test]
fn requires_elevation_respects_should_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: false,
        needs_elevation: true,
    };

    assert!(!task.requires_elevation(&ctx));
}

#[test]
fn task_phase_defaults_to_provision() {
    assert_eq!(CountingResourceTask.phase(), TaskPhase::Provision);
}

#[test]
fn resource_task_should_run_does_not_evaluate_items() {
    RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    assert!(CountingResourceTask.should_run(&ctx));
    RESOURCE_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn resource_task_run_evaluates_items_once_when_called_directly() {
    RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    let result = CountingResourceTask.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::NotApplicable(_)));
    RESOURCE_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}

#[test]
fn batch_task_should_run_does_not_evaluate_items() {
    BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    assert!(CountingBatchTask.should_run(&ctx));
    BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn batch_task_run_configured_evaluates_items_once() {
    BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    let result = CountingBatchTask.run_configured(&ctx).unwrap();
    assert!(result.is_none());
    BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}
