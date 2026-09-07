//! Shared scheduler contracts run against both modes; concurrency has its own suite.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use super::*;
use crate::engine::{TaskMeta, TaskResult, TaskStats};
use crate::infra::exec::{ExecError, ExecResult};
use crate::test_helpers::{ContextBuilder, empty_config};

mod conformance;
mod output;
mod parallel;

#[derive(Debug, Clone, Copy)]
enum Mode {
    Sequential,
    Parallel,
}

impl Mode {
    fn run(
        self,
        tasks: &[&dyn Task],
        ctx: &Context,
        log: &Arc<Logger>,
        prior: Option<&ExecutionSummary>,
    ) -> ExecutionSummary {
        let graph = ResolvedTaskGraph::resolve(tasks).unwrap();
        let assessments = tasks
            .iter()
            .map(|task| (task.task_id(), task.assess(ctx)))
            .collect();
        let summary = match (self, prior) {
            (Self::Sequential, None) => run_tasks_sequential(tasks, &graph, &assessments, ctx, log),
            (Self::Parallel, None) => run_tasks_parallel(tasks, &graph, &assessments, ctx, log),
            (Self::Sequential, Some(prior)) => {
                run_tasks_sequential_with_prior(tasks, &graph, &assessments, ctx, log, Some(prior))
            }
            (Self::Parallel, Some(prior)) => {
                run_tasks_parallel_with_prior(tasks, &graph, &assessments, ctx, log, Some(prior))
            }
        };
        let entries = log.task_entries();
        for (idx, task) in tasks.iter().enumerate() {
            let positions: Vec<_> = entries
                .iter()
                .enumerate()
                .filter_map(|(pos, entry)| (entry.task_id == task.log_key()).then_some(pos))
                .collect();
            assert_eq!(
                positions.len(),
                1,
                "{self:?}: {} recorded once",
                task.name()
            );
            for &dep_idx in graph.dependencies(idx) {
                let dependency = tasks[dep_idx].log_key();
                let dep_pos = entries
                    .iter()
                    .position(|entry| entry.task_id == dependency)
                    .unwrap();
                assert!(
                    dep_pos < positions[0],
                    "{self:?}: {} must finish after {} even when blocked",
                    task.name(),
                    tasks[dep_idx].name()
                );
            }
        }
        summary
    }
}

/// Compare dependency outcomes and user-visible records, not incidental completion order or timing.
fn conformance(case: &str, contract: impl Fn(Mode, &Context, &Arc<Logger>) -> ExecutionSummary) {
    conformance_with_verbosity(case, false, contract);
}

fn conformance_with_verbosity(
    case: &str,
    verbose: bool,
    contract: impl Fn(Mode, &Context, &Arc<Logger>) -> ExecutionSummary,
) {
    let mut previous = None;
    for mode in [Mode::Sequential, Mode::Parallel] {
        let (ctx, log, _root, _guard) = fixture(mode, verbose);
        let summary = contract(mode, &ctx, &log);
        let mut records: Vec<_> = log
            .task_entries()
            .into_iter()
            .map(|entry| {
                (
                    entry.task_id,
                    entry.name,
                    entry.status,
                    entry.message,
                    entry.actions,
                    entry.visibility,
                    entry.duration.is_some(),
                )
            })
            .collect();
        records.sort_by(|left, right| left.0.cmp(&right.0));
        let actual = (summary, records);
        if let Some(expected) = &previous {
            assert_eq!(&actual, expected, "{case}: sequential/parallel parity");
        }
        previous = Some(actual);
    }
}

fn fixture(
    mode: Mode,
    verbose: bool,
) -> (
    Context,
    Arc<Logger>,
    tempfile::TempDir,
    logging::TestDispatchGuard,
) {
    let (mut log, root, guard) = logging::isolated_logger();
    log.set_verbose(verbose);
    let log = Arc::new(log);
    let output: Arc<dyn Log> = Arc::<Logger>::clone(&log);
    let ctx = ContextBuilder::new(empty_config(root.path().to_path_buf()))
        .build()
        .with_log(output)
        .with_parallel(matches!(mode, Mode::Parallel));
    (ctx, log, root, guard)
}

#[derive(Clone)]
enum Behavior {
    Return(TaskResult),
    Error,
    PanicStr,
    PanicString,
    PanicOpaque,
    Cancel,
    Interrupted,
}

struct TestTask {
    key: &'static str,
    name: &'static str,
    deps: Vec<TaskId>,
    ordering: Vec<TaskId>,
    behavior: Behavior,
    applicable: bool,
    ran: AtomicBool,
}

impl TestTask {
    fn new(name: &'static str) -> Self {
        Self {
            key: name,
            name,
            deps: Vec::new(),
            ordering: Vec::new(),
            behavior: Behavior::Return(TaskResult::Ok),
            applicable: true,
            ran: AtomicBool::new(false),
        }
    }

    fn returning(mut self, behavior: Behavior) -> Self {
        self.behavior = behavior;
        self
    }

    fn after(mut self, tasks: &[&Self]) -> Self {
        self.deps = tasks.iter().map(|task| task.task_id()).collect();
        self
    }

    fn ordered_after(mut self, tasks: &[&Self]) -> Self {
        self.ordering = tasks.iter().map(|task| task.task_id()).collect();
        self
    }

    fn assert_ran(&self, expected: bool) {
        assert_eq!(self.ran.load(Ordering::SeqCst), expected, "{}", self.key);
    }

    fn assert_record(
        &self,
        log: &Logger,
        summary: &ExecutionSummary,
        status: TaskStatus,
        outcome: TaskOutcome,
        message: Option<&str>,
    ) {
        let entry = log
            .task_entries()
            .into_iter()
            .find(|entry| entry.task_id == self.log_key())
            .expect("task must be recorded");
        assert_eq!(entry.status, status, "{}", self.key);
        assert_eq!(entry.message.as_deref(), message, "{}", self.key);
        assert_eq!(
            summary.outcome(&self.task_id()),
            Some(outcome),
            "{}",
            self.key
        );
    }
}

impl Task for TestTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(self.name)
    }

    fn task_id(&self) -> TaskId {
        TaskId::dynamic::<Self>(self.key)
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }

    fn ordering_dependencies(&self) -> &[TaskId] {
        &self.ordering
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        self.applicable
    }

    #[allow(clippy::panic, reason = "exercise scheduler panic payload handling")]
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.ran.store(true, Ordering::SeqCst);
        match &self.behavior {
            Behavior::Return(result) => Ok(result.clone()),
            Behavior::Error => anyhow::bail!("simulated error"),
            Behavior::PanicStr => panic!("simulated panic"),
            Behavior::PanicString => std::panic::panic_any(String::from("owned panic")),
            Behavior::PanicOpaque => std::panic::panic_any(17_u8),
            Behavior::Cancel => {
                ctx.cancellation_token().cancel();
                Ok(TaskResult::Ok)
            }
            Behavior::Interrupted => Err(ExecError::Cancelled {
                command: "fixture command".to_string(),
                result: ExecResult::failure("", "", None),
            }
            .into()),
        }
    }
}
