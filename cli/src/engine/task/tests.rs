use super::*;
use crate::engine::{
    IntrinsicState, ProcessOpts, Resource, ResourceChange, ResourceResult, ResourceState, TaskStats,
};
use crate::infra::ConfigHandle;
use crate::infra::logging::{ActionCounts, TaskStatus};
use crate::test_helpers::{empty_config, make_static_context};
use anyhow::Result;
use std::any::TypeId;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

thread_local! {
    static RESOURCE_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
    static BATCH_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
    static CONFIG_RESOURCE_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
    static CONFIG_BATCH_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
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
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(ResourceState::Correct)
    }
}

/// Test-only task exercising the shared resource-task body.
#[derive(Debug)]
struct CountingResourceTask;

impl CountingResourceTask {
    fn process(ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
        run_resource_task(
            ctx,
            announce,
            Vec::<()>::new(),
            |(), _ctx| DummyResource,
            &ProcessOpts::strict("count"),
        )
    }
}

impl Task for CountingResourceTask {
    task_metadata! {
        name: "Counting resource task",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        Self::process(ctx, Some("Counting resource task"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(Self::process(ctx, None)?))
    }
}

/// Test-only task exercising the shared batch-resource-task body.
#[derive(Debug)]
struct CountingBatchTask;

impl CountingBatchTask {
    fn process(ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
        run_batch_resource_task(
            ctx,
            announce,
            Vec::<()>::new(),
            |(), _ctx| DummyResource,
            |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
            |_resource, _cache| Ok(ResourceState::Correct),
            &ProcessOpts::strict("count"),
        )
    }
}

impl Task for CountingBatchTask {
    task_metadata! {
        name: "Counting batch task",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        Self::process(ctx, Some("Counting batch task"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(Self::process(ctx, None)?))
    }
}

/// Test-only config-backed task exercising the shared resource-task body.
#[derive(Debug)]
struct CountingConfigResourceTask {
    config: ConfigHandle<Vec<()>>,
}

impl CountingConfigResourceTask {
    const fn new(config: ConfigHandle<Vec<()>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        let items = self.config.read().to_vec();
        CONFIG_RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
        run_resource_task(
            ctx,
            announce,
            items,
            |(), _ctx| DummyResource,
            &ProcessOpts::strict("count"),
        )
    }
}

impl Task for CountingConfigResourceTask {
    task_metadata! {
        name: "Counting config resource task",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.process(ctx, Some("Counting config resource task"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)?))
    }
}

/// Test-only config-backed task exercising the shared batch body.
#[derive(Debug)]
struct CountingConfigBatchTask {
    config: ConfigHandle<Vec<()>>,
}

impl CountingConfigBatchTask {
    const fn new(config: ConfigHandle<Vec<()>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<Option<TaskResult>> {
        let items = self.config.read().to_vec();
        CONFIG_BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get().saturating_add(1)));
        run_batch_resource_task(
            ctx,
            announce,
            items,
            |(), _ctx| DummyResource,
            |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
            |_resource, _cache| Ok(ResourceState::Correct),
            &ProcessOpts::strict("count"),
        )
    }
}

impl Task for CountingConfigBatchTask {
    task_metadata! {
        name: "Counting config batch task",
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.process(ctx, Some("Counting config batch task"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(configured_task_result(self.process(ctx, None)?))
    }
}

/// A mock task for testing `execute()`.
struct MockTask {
    name: &'static str,
    should_run: bool,
    result: Result<TaskResult, String>,
}

impl Task for MockTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(self.name)
    }
    fn should_run(&self, _ctx: &Context) -> bool {
        self.should_run
    }
    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.result.clone().map_err(|s| anyhow::anyhow!("{s}"))
    }
}

struct CheckPassedTask;

impl Task for CheckPassedTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("check-passed")
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::CheckPassed)
    }
}

struct GatedTask {
    ran: Arc<AtomicBool>,
    should_run: bool,
    needs_elevation: bool,
}

impl Task for GatedTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("gated-task")
    }
    fn should_run(&self, _ctx: &Context) -> bool {
        self.should_run
    }
    fn needs_elevation(&self, _ctx: &Context) -> bool {
        self.needs_elevation
    }
    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.ran.store(true, Ordering::SeqCst);
        Ok(TaskResult::Ok)
    }
}

#[derive(Default)]
struct DelegationCalls {
    should_run: AtomicUsize,
    run_configured: AtomicUsize,
    needs_elevation: AtomicUsize,
    run: AtomicUsize,
}

struct DelegatedTask {
    calls: Arc<DelegationCalls>,
    deps: Vec<TaskId>,
}

impl Task for DelegatedTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("delegated-task")
            .with_selector("delegated")
            .with_visibility(TaskVisibility::Internal)
            .with_update_only(true)
    }

    fn task_id(&self) -> TaskId {
        TaskId::Dynamic(17)
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        self.calls.should_run.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn run_configured(&self, _ctx: &Context) -> Result<Option<TaskResult>> {
        self.calls.run_configured.fetch_add(1, Ordering::SeqCst);
        Ok(Some(TaskResult::Skipped("configured".to_string())))
    }

    fn needs_elevation(&self, _ctx: &Context) -> bool {
        self.calls.needs_elevation.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.calls.run.fetch_add(1, Ordering::SeqCst);
        Ok(TaskResult::Failed("direct".to_string()))
    }
}

#[test]
fn task_with_extra_deps_forwards_task_contract_and_deduplicates_dependencies() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let calls = Arc::new(DelegationCalls::default());
    let existing = TaskId::Type(TypeId::of::<u8>());
    let additional = TaskId::Type(TypeId::of::<u16>());
    let task = TaskWithExtraDeps::new(
        Box::new(DelegatedTask {
            calls: Arc::clone(&calls),
            deps: vec![existing.clone(), existing.clone()],
        }),
        &[existing.clone(), additional.clone(), additional.clone()],
    );

    assert_eq!(task.name(), "delegated-task");
    assert_eq!(task.selector(), "delegated");
    assert_eq!(task.visibility(), TaskVisibility::Internal);
    assert!(task.update_only());
    assert_eq!(task.task_id(), TaskId::Dynamic(17));
    assert_eq!(task.dependencies(), &[existing, additional]);
    assert!(task.should_run(&ctx));
    assert!(task.needs_elevation(&ctx));
    assert!(requires_elevation(&task, &ctx));
    assert!(matches!(
        task.run_configured(&ctx).unwrap(),
        Some(TaskResult::Skipped(reason)) if reason == "configured"
    ));
    assert!(matches!(
        task.run(&ctx).unwrap(),
        TaskResult::Failed(reason) if reason == "direct"
    ));

    assert_eq!(calls.should_run.load(Ordering::SeqCst), 2);
    assert_eq!(calls.needs_elevation.load(Ordering::SeqCst), 2);
    assert_eq!(calls.run_configured.load(Ordering::SeqCst), 1);
    assert_eq!(calls.run.load(Ordering::SeqCst), 1);
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
fn execute_records_check_passed_task_as_passed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);

    execute(&CheckPassedTask, &ctx);

    let entries = log.task_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, TaskStatus::Passed);
    assert_eq!(entries[0].name, "check-passed");
}

#[test]
fn execute_records_ok_task_with_message() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "ok-task",
        should_run: true,
        result: Ok(TaskStats::changed_with_message("created config file").finish()),
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
            message: None,
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
            message: None,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::DryRun);

    let entry = &log.task_entries()[0];
    assert_eq!(entry.actions.applied, 0);
    assert_eq!(entry.actions.planned, 4);
}

#[test]
fn execute_records_unquantified_dry_run_without_planned_actions() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ctx = ctx.with_dry_run(true);
    let task = MockTask {
        name: "dry-task",
        should_run: true,
        result: Ok(TaskResult::DryRun),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::DryRun);

    let entry = &log.task_entries()[0];
    assert_eq!(entry.actions, ActionCounts::default());
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
            message: None,
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
            message: None,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Skipped);
    assert_eq!(log.task_entries()[0].actions.skipped, 3);
}

#[test]
fn execute_preserves_batch_failure_when_cancellation_was_requested_separately() {
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
            message: None,
        })),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Failed);
    assert_eq!(log.failure_count(), 1);
    assert_eq!(log.task_entries()[0].actions.failed, 1);
}

#[test]
fn execute_detects_cancellation_through_resource_error_wrappers() {
    struct WrappedCancellationTask;

    impl Task for WrappedCancellationTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("wrapped-cancellation")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            let exec = crate::infra::exec::ExecError::Cancelled {
                command: "wrapped command".to_string(),
                result: crate::infra::exec::ExecResult::failure("", "", None),
            };
            let resource = crate::engine::resource::ResourceError::from(anyhow::Error::new(
                crate::engine::resource::ResourceError::from(exec),
            ));
            Err(resource.into())
        }
    }

    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);

    assert_eq!(execute(&WrappedCancellationTask, &ctx), TaskStatus::Skipped);
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
    assert_eq!(log.task_entries()[0].actions.failed, 1);
}

#[test]
fn execute_records_dry_run_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ctx = ctx.with_dry_run(true);
    let task = MockTask {
        name: "dry-task",
        should_run: true,
        result: Ok(TaskStats::changed().finish()),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
    assert_eq!(log.task_entries()[0].actions.planned, 1);
}

#[test]
fn execute_checks_applicability_before_running_task() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ran = Arc::new(AtomicBool::new(false));
    let task = GatedTask {
        ran: Arc::clone(&ran),
        should_run: false,
        needs_elevation: false,
    };

    execute(&task, &ctx);

    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn requires_elevation_respects_prediction_and_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = Arc::new(AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: true,
        needs_elevation: true,
    };

    assert!(requires_elevation(&task, &ctx));
    assert!(!requires_elevation(&task, &ctx.with_dry_run(true)));
}

#[test]
fn requires_elevation_respects_prediction() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = Arc::new(AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: true,
        needs_elevation: false,
    };

    assert!(!requires_elevation(&task, &ctx));
}

#[test]
fn requires_elevation_respects_should_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let ran = Arc::new(AtomicBool::new(false));
    let task = GatedTask {
        ran,
        should_run: false,
        needs_elevation: true,
    };

    assert!(!requires_elevation(&task, &ctx));
}

#[test]
fn assessment_evaluates_applicability_once_for_unelevated_tasks() {
    struct CountingGate {
        should_run_calls: Arc<AtomicUsize>,
    }

    impl Task for CountingGate {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("counting-gate")
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.should_run_calls.fetch_add(1, Ordering::SeqCst);
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let should_run_calls = Arc::new(AtomicUsize::new(0));
    let task = CountingGate {
        should_run_calls: Arc::clone(&should_run_calls),
    };

    assert!(!requires_elevation(&task, &ctx));
    assert_eq!(
        should_run_calls.load(Ordering::SeqCst),
        1,
        "the unified assessment must evaluate applicability exactly once"
    );
}

#[test]
fn precomputed_assessment_is_reused_during_execution() {
    struct CountingAssessment {
        should_run_calls: Arc<AtomicUsize>,
        elevation_calls: Arc<AtomicUsize>,
        ran: Arc<AtomicBool>,
    }

    impl Task for CountingAssessment {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("counting-assessment")
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            self.should_run_calls.fetch_add(1, Ordering::SeqCst);
            true
        }

        fn needs_elevation(&self, _ctx: &Context) -> bool {
            self.elevation_calls.fetch_add(1, Ordering::SeqCst);
            false
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let should_run_calls = Arc::new(AtomicUsize::new(0));
    let elevation_calls = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(AtomicBool::new(false));
    let task = CountingAssessment {
        should_run_calls: Arc::clone(&should_run_calls),
        elevation_calls: Arc::clone(&elevation_calls),
        ran: Arc::clone(&ran),
    };

    let assessment = task.assess(&ctx);
    assert!(!assessment.requires_elevation());
    assert_eq!(execute_assessed(&task, &assessment, &ctx), TaskStatus::Ok);

    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(should_run_calls.load(Ordering::SeqCst), 1);
    assert_eq!(elevation_calls.load(Ordering::SeqCst), 1);
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
fn batch_task_run_configured_evaluates_items_once() {
    BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);

    let result = CountingBatchTask.run_configured(&ctx).unwrap();
    assert!(result.is_none());
    BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}

#[test]
fn config_resource_task_run_evaluates_snapshot_items_once() {
    CONFIG_RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let task = CountingConfigResourceTask::new(ConfigHandle::new(Vec::new()));

    let result = task.run(&ctx).unwrap();
    assert!(matches!(result, TaskResult::NotApplicable(_)));
    CONFIG_RESOURCE_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}

#[test]
fn config_batch_task_run_configured_evaluates_snapshot_items_once() {
    CONFIG_BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _) = make_static_context(config);
    let task = CountingConfigBatchTask::new(ConfigHandle::new(Vec::new()));

    let result = task.run_configured(&ctx).unwrap();
    assert!(result.is_none());
    CONFIG_BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
}
