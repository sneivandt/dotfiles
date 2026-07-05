use super::*;
use crate::logging::TaskStatus;
use crate::platform::Platform;
use crate::resources::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use anyhow::Result;
use std::cell::Cell;
use std::path::PathBuf;
use test_helpers::{empty_config, make_static_context};

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
        phase: TaskPhase::Provision,
        domain: Domain::General,
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
        phase: TaskPhase::Provision,
        domain: Domain::General,
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
    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
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

    fn domain(&self) -> Domain {
        Domain::Validation
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Ok)
    }
}

struct PolicyTask {
    policies: &'static [ExecutionPolicy],
    ran: std::sync::Arc<std::sync::atomic::AtomicBool>,
    should_run: bool,
    needs_elevation: bool,
}

impl Task for PolicyTask {
    fn name(&self) -> &'static str {
        "policy-task"
    }
    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }
    fn execution_policies(&self) -> &[ExecutionPolicy] {
        self.policies
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

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
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
}

#[test]
fn execute_applies_platform_policy_before_running_task() {
    const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::PlatformSupported(
        "Windows",
        Platform::is_windows,
    )];
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = PolicyTask {
        policies: POLICIES,
        ran: std::sync::Arc::clone(&ran),
        should_run: true,
        needs_elevation: false,
    };

    execute(&task, &ctx);

    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn requires_elevation_respects_policy_and_dry_run() {
    const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::RequiresElevation];
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = PolicyTask {
        policies: POLICIES,
        ran,
        should_run: true,
        needs_elevation: true,
    };

    assert!(task.requires_elevation(&ctx));
    assert!(!task.requires_elevation(&ctx.with_dry_run(true)));
}

#[test]
fn requires_elevation_respects_platform_policy() {
    const POLICIES: &[ExecutionPolicy] = &[
        ExecutionPolicy::PlatformSupported("Windows", Platform::is_windows),
        ExecutionPolicy::RequiresElevation,
    ];
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = PolicyTask {
        policies: POLICIES,
        ran,
        should_run: true,
        needs_elevation: true,
    };

    assert!(!task.requires_elevation(&ctx));
}

#[test]
fn requires_elevation_respects_should_run() {
    const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::RequiresElevation];
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task = PolicyTask {
        policies: POLICIES,
        ran,
        should_run: false,
        needs_elevation: true,
    };

    assert!(!task.requires_elevation(&ctx));
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
fn batch_task_run_if_applicable_evaluates_items_once() {
    BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    let result = CountingBatchTask.run_if_applicable(&ctx).unwrap();
    assert!(result.is_none());
    BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}

// ------------------------------------------------------------------
// Task registration completeness
// ------------------------------------------------------------------

/// Guard against forgetting to register a new task.
///
/// When you add a new task to the codebase, add it to
/// `all_install_tasks()` and bump the expected count here.
#[test]
fn all_install_tasks_count() {
    let tasks = all_install_tasks();
    assert_eq!(
        tasks.len(),
        23,
        "expected 23 install tasks — did you add a new task without updating \
             all_install_tasks()? Update the registration list and this test."
    );
}

#[test]
fn all_uninstall_tasks_count() {
    let tasks = all_uninstall_tasks();
    assert_eq!(
        tasks.len(),
        3,
        "expected 3 uninstall tasks — update all_uninstall_tasks() and this test."
    );
}
