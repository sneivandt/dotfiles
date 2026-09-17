use std::sync::{Arc, Barrier, Mutex};

use crate::engine::{
    BatchCompletion, BatchReport, Context, IntrinsicState, ProcessOpts, RemovableResource,
    Resource, ResourceChange, ResourceResult, ResourceState, Task, TaskMeta, TaskResult,
    process_resources, process_resources_remove,
};
use crate::infra::exec::{ExecError, ExecResult};
use crate::infra::logging::TaskStatus;
use crate::test_helpers::empty_config;

use super::test_context;

#[derive(Debug, Clone, Copy)]
enum Behavior {
    Apply,
    Fail,
    Interrupt,
    CancelAfterApply,
    Current,
    ProbeFail,
}

struct ProbeResource {
    index: usize,
    behavior: Behavior,
    ctx: Context,
    calls: Arc<Mutex<Vec<usize>>>,
    barrier: Option<Arc<Barrier>>,
}

impl Resource for ProbeResource {
    fn description(&self) -> String {
        format!("item-{}", self.index)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.calls.lock().unwrap().push(self.index);
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        match self.behavior {
            Behavior::Fail => Err(crate::engine::resource::ResourceError::permission_denied(
                self.description(),
            )),
            Behavior::Interrupt => Err(ExecError::Cancelled {
                command: self.description(),
                result: ExecResult::failure("", "", None),
            }
            .into()),
            Behavior::CancelAfterApply => {
                self.ctx.cancellation_token().cancel();
                Ok(ResourceChange::Applied)
            }
            Behavior::Apply | Behavior::Current | Behavior::ProbeFail => {
                Ok(ResourceChange::Applied)
            }
        }
    }
}

impl IntrinsicState for ProbeResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        match self.behavior {
            Behavior::Current => Ok(ResourceState::Correct),
            Behavior::ProbeFail => Err(std::io::Error::other("probe failed").into()),
            Behavior::Apply | Behavior::Fail | Behavior::Interrupt | Behavior::CancelAfterApply => {
                Ok(ResourceState::Missing)
            }
        }
    }
}

impl RemovableResource for ProbeResource {
    fn remove_when_missing(&self) -> bool {
        true
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.apply()
    }
}

struct BatchTask {
    behaviors: Vec<Behavior>,
    calls: Arc<Mutex<Vec<usize>>>,
    barrier: Option<Arc<Barrier>>,
    remove: bool,
    opts: ProcessOpts,
}

impl BatchTask {
    fn new(behaviors: &[Behavior]) -> Self {
        Self {
            behaviors: behaviors.to_vec(),
            calls: Arc::default(),
            barrier: None,
            remove: false,
            opts: ProcessOpts::strict("apply"),
        }
    }
}

impl Task for BatchTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("batch report")
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let resources = self
            .behaviors
            .iter()
            .enumerate()
            .map(|(index, &behavior)| ProbeResource {
                index,
                behavior,
                ctx: ctx.clone(),
                calls: Arc::clone(&self.calls),
                barrier: self.barrier.clone(),
            });
        if self.remove {
            process_resources_remove(ctx, resources, "remove")
        } else {
            process_resources(ctx, resources, &self.opts)
        }
    }
}

#[test]
fn stopped_batches_retain_counts_and_typed_causes() {
    for (behavior, dry_run, completion, changed, failed, interrupted, calls) in [
        (
            Behavior::Fail,
            false,
            BatchCompletion::Failed,
            1,
            1,
            0,
            vec![0, 1],
        ),
        (
            Behavior::ProbeFail,
            false,
            BatchCompletion::Failed,
            1,
            1,
            0,
            vec![0],
        ),
        (
            Behavior::ProbeFail,
            true,
            BatchCompletion::Failed,
            1,
            1,
            0,
            vec![],
        ),
        (
            Behavior::Interrupt,
            false,
            BatchCompletion::Interrupted,
            1,
            0,
            1,
            vec![0, 1],
        ),
    ] {
        let (ctx, _) = test_context(empty_config("/fixture".into()));
        let ctx = ctx.with_dry_run(dry_run);
        let task = BatchTask::new(&[Behavior::Apply, behavior, Behavior::Apply]);
        let error = task.run(&ctx).unwrap_err().context("task context");
        let report = error.downcast_ref::<BatchReport>().unwrap();
        assert_eq!(
            report.completion(),
            completion,
            "{behavior:?}, dry={dry_run}"
        );
        assert_eq!(report.stats().changed_count(), changed);
        assert_eq!(report.stats().failed_count(), failed);
        assert_eq!(report.interrupted_count(), interrupted);
        assert_eq!(report.not_attempted_count(), 1);
        assert_eq!(*task.calls.lock().unwrap(), calls);
        assert!(
            error
                .downcast_ref::<crate::engine::resource::ResourceError>()
                .is_some(),
            "batch metadata must not hide the original resource error"
        );
    }
}

#[test]
fn task_records_keep_partial_failure_and_interruption_counts() {
    for remove in [false, true] {
        for (behavior, status, failed, interrupted, not_attempted) in [
            (Behavior::Fail, TaskStatus::Failed, 1, 0, 1),
            (Behavior::Interrupt, TaskStatus::Interrupted, 0, 1, 1),
            (Behavior::CancelAfterApply, TaskStatus::Interrupted, 0, 0, 1),
        ] {
            let (ctx, log) = test_context(empty_config("/fixture".into()));
            let mut task = BatchTask::new(&[Behavior::Apply, behavior, Behavior::Apply]);
            task.remove = remove;
            assert_eq!(crate::engine::execute(&task, &ctx), status);
            let entry = log.task_entries().pop().unwrap();
            let changed = if matches!(behavior, Behavior::CancelAfterApply) {
                2
            } else {
                1
            };
            assert_eq!(entry.actions.applied, changed);
            assert_eq!(entry.actions.failed, failed);
            assert_eq!(entry.actions.interrupted, interrupted);
            assert_eq!(entry.actions.not_attempted, not_attempted);
            assert!(entry.message.unwrap().contains("1 not attempted"));
        }
    }
}

#[test]
fn a_fully_processed_batch_is_not_relabelled_by_late_cancellation() {
    let (ctx, log) = test_context(empty_config("/fixture".into()));
    let task = BatchTask::new(&[Behavior::Current, Behavior::CancelAfterApply]);
    assert_eq!(crate::engine::execute(&task, &ctx), TaskStatus::Changed);
    assert!(ctx.is_cancelled());
    assert_eq!(log.task_entries().last().unwrap().actions.applied, 1);
}

#[test]
fn stopped_preview_records_plans_without_applied_changes() {
    let (ctx, log) = test_context(empty_config("/fixture".into()));
    let task = BatchTask::new(&[Behavior::Apply, Behavior::ProbeFail, Behavior::Apply]);
    assert_eq!(
        crate::engine::execute(&task, &ctx.with_dry_run(true)),
        TaskStatus::Failed
    );
    let entry = log.task_entries().pop().unwrap();
    assert_eq!(entry.actions.applied, 0);
    assert_eq!(entry.actions.planned, 1);
    assert_eq!(entry.actions.failed, 1);
    assert_eq!(entry.actions.not_attempted, 1);
    assert!(task.calls.lock().unwrap().is_empty());
}

#[test]
fn lenient_failure_is_retained_when_later_work_is_interrupted() {
    for behavior in [Behavior::Interrupt, Behavior::CancelAfterApply] {
        let (ctx, log) = test_context(empty_config("/fixture".into()));
        let mut task = BatchTask::new(&[Behavior::Fail, behavior, Behavior::Apply]);
        task.opts = ProcessOpts::lenient("apply");
        assert_eq!(crate::engine::execute(&task, &ctx), TaskStatus::Failed);
        let entry = log.task_entries().pop().unwrap();
        assert_eq!(entry.actions.failed, 1);
        assert_eq!(entry.actions.not_attempted, 1);
        assert_eq!(
            entry.actions.applied,
            u32::from(matches!(behavior, Behavior::CancelAfterApply))
        );
        assert_eq!(
            entry.actions.interrupted,
            u32::from(matches!(behavior, Behavior::Interrupt))
        );
        assert_eq!(*task.calls.lock().unwrap(), [0, 1]);
    }
}

#[test]
fn parallel_failure_joins_and_accounts_for_other_in_flight_work() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    for behavior in [Behavior::Apply, Behavior::Interrupt] {
        let (ctx, log) = test_context(empty_config("/fixture".into()));
        let ctx = ctx.with_parallel(true);
        let mut task = BatchTask::new(&[behavior, Behavior::Fail]);
        task.barrier = Some(Arc::new(Barrier::new(2)));
        let status = pool.install(|| crate::engine::execute(&task, &ctx));
        assert_eq!(
            status,
            TaskStatus::Failed,
            "real failures beat interruption"
        );
        let entry = log.task_entries().pop().unwrap();
        assert_eq!(task.calls.lock().unwrap().len(), 2);
        assert_eq!(entry.actions.failed, 1);
        assert_eq!(entry.actions.not_attempted, 0);
        assert_eq!(
            entry.actions.applied,
            u32::from(matches!(behavior, Behavior::Apply))
        );
        assert_eq!(
            entry.actions.interrupted,
            u32::from(matches!(behavior, Behavior::Interrupt))
        );
        assert!(entry.message.unwrap().contains("permission denied"));
    }
}
