use super::*;
use crate::engine::{
    IntrinsicState, ProcessOpts, Resource, ResourceChange, ResourceResult, ResourceState, TaskStats,
};
use crate::infra::ConfigHandle;
use crate::infra::logging::{ActionCounts, TaskStatus};
use crate::test_helpers::{empty_config, make_static_context, numeric_task_id};
use anyhow::Result;
use std::any::TypeId;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// Exercise both resource-task bodies with either direct or config-backed items.
struct CountingResourceTask {
    config: Option<ConfigHandle<Vec<()>>>,
    batch: bool,
    item_evaluations: AtomicUsize,
}

impl Task for CountingResourceTask {
    task_metadata! {
        name: "Counting resource task",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items = self
            .config
            .as_ref()
            .map_or_else(Vec::new, |config| config.read().to_vec());
        self.item_evaluations.fetch_add(1, Ordering::SeqCst);
        if self.batch {
            run_batch_resource_task(
                ctx,
                items,
                |(), _ctx| DummyResource,
                |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
                |_resource, _cache| Ok(ResourceState::Correct),
                &ProcessOpts::strict("count"),
            )
        } else {
            run_resource_task(
                ctx,
                items,
                |(), _ctx| DummyResource,
                &ProcessOpts::strict("count"),
            )
        }
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
    needs_elevation: AtomicUsize,
    run: AtomicUsize,
}

struct DelegatedTask {
    calls: Arc<DelegationCalls>,
    deps: Vec<TaskId>,
    ordering_deps: Vec<TaskId>,
}

impl Task for DelegatedTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("delegated-task")
            .with_selector("delegated")
            .with_visibility(TaskVisibility::Internal)
            .with_update_only(true)
    }

    fn task_id(&self) -> TaskId {
        numeric_task_id(17)
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }

    fn ordering_dependencies(&self) -> &[TaskId] {
        &self.ordering_deps
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        self.calls.should_run.fetch_add(1, Ordering::SeqCst);
        true
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
            ordering_deps: Vec::new(),
        }),
        &[existing.clone(), additional.clone(), additional.clone()],
        &[],
    );

    assert_eq!(task.name(), "delegated-task");
    assert_eq!(task.selector(), "delegated");
    assert_eq!(task.visibility(), TaskVisibility::Internal);
    assert!(task.update_only());
    assert_eq!(task.task_id(), numeric_task_id(17));
    assert_eq!(task.dependencies(), &[existing, additional]);
    assert!(task.should_run(&ctx));
    assert!(task.needs_elevation(&ctx));
    assert!(requires_elevation(&task, &ctx));
    assert!(matches!(
        task.run(&ctx).unwrap(),
        TaskResult::Failed(reason) if reason == "direct"
    ));

    assert_eq!(calls.should_run.load(Ordering::SeqCst), 2);
    assert_eq!(calls.needs_elevation.load(Ordering::SeqCst), 2);
    assert_eq!(calls.run.load(Ordering::SeqCst), 1);
}

#[test]
fn task_with_extra_deps_merges_both_edge_kinds() {
    let calls = Arc::new(DelegationCalls::default());
    let blocking = TaskId::Type(TypeId::of::<u8>());
    let existing = TaskId::Type(TypeId::of::<u16>());
    let additional = TaskId::Type(TypeId::of::<u32>());
    let task = TaskWithExtraDeps::new(
        Box::new(DelegatedTask {
            calls,
            deps: vec![blocking.clone()],
            ordering_deps: vec![existing.clone(), existing.clone()],
        }),
        &[],
        &[existing.clone(), additional.clone(), additional.clone()],
    );

    assert_eq!(task.dependencies(), &[blocking]);
    assert_eq!(task.ordering_dependencies(), &[existing, additional]);
    assert_eq!(task.task_id(), numeric_task_id(17));
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
        result: Ok(TaskResult::skipped("not needed")),
    };

    execute(&task, &ctx);
    assert_eq!(log.failure_count(), 0);
}

#[test]
fn strict_completion_escalates_unmet_task_skip() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let ctx = ctx.with_require_complete(true);
    let task = MockTask {
        name: "missing-tool",
        should_run: true,
        result: Ok(TaskResult::unmet("required tool unavailable")),
    };

    assert_eq!(execute(&task, &ctx), TaskStatus::Failed);
    assert_eq!(log.failure_count(), 1);
}

#[test]
fn execute_records_batch_action_counts() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, log) = make_static_context(config);
    let task = MockTask {
        name: "batch-task",
        should_run: true,
        result: Ok(TaskResult::Batch(TaskStats::from_counts(3, 5, 2, 0))),
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
        result: Ok(TaskResult::Batch(TaskStats::from_counts(4, 1, 0, 0))),
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
        result: Ok(TaskResult::Batch(TaskStats::from_counts(1, 0, 2, 3))),
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
        result: Ok(TaskResult::Batch(TaskStats::from_counts(0, 2, 3, 0))),
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
        result: Ok(TaskResult::Batch(TaskStats::from_counts(0, 0, 0, 1))),
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

    assert_eq!(
        execute(&WrappedCancellationTask, &ctx),
        TaskStatus::Interrupted
    );
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
    assert_eq!(
        execute_assessed(&task, &assessment, &ctx).status,
        TaskStatus::Ok
    );

    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(should_run_calls.load(Ordering::SeqCst), 1);
    assert_eq!(elevation_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn resource_task_bodies_evaluate_items_once() {
    for (name, batch, config_backed) in [
        ("direct resource", false, false),
        ("direct batch", true, false),
        ("config resource", false, true),
        ("config batch", true, true),
    ] {
        let (ctx, _) = make_static_context(empty_config("/fixture".into()));
        let task = CountingResourceTask {
            config: config_backed.then(|| ConfigHandle::new(Vec::new())),
            batch,
            item_evaluations: AtomicUsize::new(0),
        };

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)), "{name}");
        assert_eq!(task.item_evaluations.load(Ordering::SeqCst), 1, "{name}");
    }
}

#[test]
fn persistent_task_identity_preserves_dynamic_kind_and_wrapper_identity() {
    struct IdentityTask {
        id: TaskId,
    }
    impl Task for IdentityTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("same display name")
        }
        fn task_id(&self) -> TaskId {
            self.id.clone()
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }
    let named = IdentityTask {
        id: TaskId::dynamic::<IdentityTask>("5"),
    };
    let other_key = IdentityTask {
        id: TaskId::dynamic::<IdentityTask>("6"),
    };
    assert_ne!(named.task_id(), numeric_task_id(5));
    assert_ne!(named.log_key(), other_key.log_key());
    assert_eq!(
        named.log_key(),
        format!("{}#named:5", std::any::type_name::<IdentityTask>())
    );
    let key = named.log_key();
    let decorated = TaskWithExtraDeps::new(Box::new(named), &[], &[]);
    assert_eq!(decorated.log_key(), key);
}
