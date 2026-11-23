use super::*;

// ── run_tasks_to_completion ───────────────────────────

#[test]
fn run_executes_tasks_in_dependency_order() {
    let fixture = ExecutionFixture::with_parallel(false);
    let second = fixture.task("second", 2).depends_on(&[1]);
    let first = fixture.task("first", 1);

    fixture.run(&[&second, &first]).expect("run should succeed");

    assert_eq!(
        fixture.entries(),
        ["first", "second"],
        "list order is not execution order; dependency edges are"
    );
}

#[test]
fn static_dependencies_complete_before_dependents_and_restart() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Prerequisite(Arc<AtomicBool>);
    impl Task for Prerequisite {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("prerequisite")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.0.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct Dependent {
        prerequisite_completed: Arc<AtomicBool>,
        completed: Arc<AtomicBool>,
    }
    impl Task for Dependent {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("dependent")
        }

        crate::engine::task_deps![Prerequisite];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            if !self.prerequisite_completed.load(Ordering::SeqCst) {
                return Ok(TaskResult::Failed(
                    "dependent started before prerequisite completed".into(),
                ));
            }
            self.completed.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    for parallel in [false, true] {
        for restart in [false, true] {
            let prerequisite = Prerequisite(Arc::new(AtomicBool::new(false)));
            let dependent = Dependent {
                prerequisite_completed: Arc::clone(&prerequisite.0),
                completed: Arc::new(AtomicBool::new(false)),
            };
            let fixture = ExecutionFixture::with_parallel(parallel);
            let mut remaining = fixture.task("remaining", 3);
            remaining.dependencies = vec![dependent.task_id()];
            let tasks: [&dyn Task; 3] = [&remaining, &dependent, &prerequisite];
            let restarted = std::cell::Cell::new(false);

            if restart {
                fixture.run_with_restart(
                    &tasks,
                    dependent.task_id(),
                    || true,
                    || {
                        assert!(prerequisite.0.load(Ordering::SeqCst));
                        assert!(dependent.completed.load(Ordering::SeqCst));
                        assert!(fixture.entries().is_empty());
                        restarted.set(true);
                    },
                )
            } else {
                fixture.run(&tasks)
            }
            .unwrap_or_else(|error| panic!("parallel={parallel}, restart={restart}: {error:#}"));

            assert!(dependent.completed.load(Ordering::SeqCst));
            assert_eq!(restarted.get(), restart);
            assert_eq!(
                fixture.entries(),
                if restart { vec![] } else { vec!["remaining"] },
                "parallel={parallel}, restart={restart}"
            );
        }
    }
}

#[test]
fn run_reports_failure_count_when_a_task_fails() {
    let fixture = ExecutionFixture::with_parallel(false);
    let ok = fixture.task("ok", 1);
    let bad = fixture.task("bad", 2).failing();

    let error = fixture
        .run(&[&ok, &bad])
        .expect_err("a failing task must fail the run");

    assert!(
        error.downcast_ref::<TaskFailures>().is_some(),
        "failures should surface as TaskFailures, got: {error:#}"
    );
}

#[test]
fn run_blocks_dependents_of_a_failed_task() {
    let fixture = ExecutionFixture::with_parallel(false);
    let bad = fixture.task("bad", 1).failing();
    let dependent = fixture.task("dependent", 2).depends_on(&[1]);

    let _error = fixture.run(&[&bad, &dependent]).unwrap_err();

    assert_eq!(
        fixture.entries(),
        ["bad"],
        "a task whose prerequisite failed must not run"
    );
}

#[test]
fn run_rejects_a_cyclic_graph_before_executing_anything() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let a = fixture.task("a", 1).depends_on(&[2]);
        let b = fixture.task("b", 2).depends_on(&[1]);

        let error = fixture
            .run(&[&a, &b])
            .expect_err("a cycle must be rejected");

        assert_eq!(
            error.to_string(),
            "dependency cycle: a -> b -> a detected in task graph",
            "parallel={parallel}"
        );
        assert!(
            fixture.entries().is_empty(),
            "no task may run when graph validation fails: parallel={parallel}"
        );
    }
}

#[test]
fn run_reports_interruption_without_dispatch_when_already_cancelled() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let a = fixture.task("a", 1);
        fixture.ctx.cancellation_token().cancel();

        let error = fixture
            .run(&[&a])
            .expect_err("cancelled work must not succeed");

        assert!(error.is::<CommandInterrupted>(), "{error:#}");
        assert!(
            fixture.entries().is_empty(),
            "cancellation before dispatch must skip every task"
        );
    }
}

#[test]
fn typed_task_cancellation_interrupts_the_command_without_a_global_signal() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let interrupted = fixture.task("interrupted", 1).interrupted();
        let dependent = fixture.task("dependent", 2).depends_on(&[1]);

        let error = fixture
            .run(&[&interrupted, &dependent])
            .expect_err("typed interruption must reach command completion");

        assert!(error.is::<CommandInterrupted>(), "{error:#}");
        assert!(!fixture.ctx.is_cancelled(), "no global token was set");
        assert_eq!(fixture.entries(), ["interrupted"]);
    }
}

#[test]
fn partial_batch_cancellation_reaches_command_completion_with_its_counts() {
    use crate::engine::{
        ProcessOpts, Resource, ResourceChange, ResourceResult, ResourceState,
        process_resources_with_cache,
    };
    use crate::infra::logging::{ActionCounts, TaskStatus};

    struct BatchResource {
        index: u8,
        trace: Trace,
    }
    impl Resource for BatchResource {
        fn description(&self) -> String {
            format!("resource {}", self.index)
        }

        fn apply(&self) -> ResourceResult<ResourceChange> {
            self.trace.lock().unwrap().push(self.description());
            if self.index == 1 {
                Err(cancelled_command().into())
            } else {
                Ok(ResourceChange::Applied)
            }
        }
    }

    struct BatchTask(Trace);
    impl Task for BatchTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("batch")
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            process_resources_with_cache(
                ctx,
                (0..3).map(|index| BatchResource {
                    index,
                    trace: Arc::clone(&self.0),
                }),
                &(),
                |_, ()| Ok(ResourceState::Missing),
                &ProcessOpts::strict("install").sequential(),
            )
            .map_err(|error| error.context("processing batch"))
        }
    }

    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let task = BatchTask(Arc::clone(&fixture.trace));

        let error = fixture.run(&[&task]).unwrap_err();

        assert!(error.is::<CommandInterrupted>(), "{error:#}");
        assert!(
            !fixture.ctx.is_cancelled(),
            "the typed error must stand on its own"
        );
        assert_eq!(fixture.entries(), ["resource 0", "resource 1"]);
        let recorded = fixture.log.task_entries();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].status, TaskStatus::Interrupted);
        assert_eq!(
            recorded[0].actions,
            ActionCounts {
                applied: 1,
                interrupted: 1,
                not_attempted: 1,
                ..ActionCounts::default()
            }
        );
    }
}

#[test]
fn real_failures_win_over_cancellation_without_counting_it_as_failure() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let failed = fixture.task("failed", 1).failing();
        let interrupted = fixture.task("interrupted", 2).interrupted();

        let error = fixture
            .run(&[&failed, &interrupted])
            .expect_err("a failure and an interruption cannot succeed");

        assert!(error.is::<TaskFailures>(), "{error:#}");
        assert_eq!(error.to_string(), "1 task(s) failed");
        assert_eq!(fixture.entries().len(), 2);
    }
}

#[test]
fn cancellation_does_not_hide_an_unrelated_failure_from_the_same_task() {
    let fixture = ExecutionFixture::with_parallel(false);
    let failed = fixture.task("failed", 1).cancelling().failing();

    let error = fixture
        .run(&[&failed])
        .expect_err("a real failure still wins when the token is set");

    assert!(error.is::<TaskFailures>(), "{error:#}");
    assert_eq!(error.to_string(), "1 task(s) failed");
}

#[test]
fn completed_task_stays_satisfied_but_cancelled_command_does_not_succeed() {
    let fixture = ExecutionFixture::with_parallel(false);
    let task = fixture.task("completed", 1).cancelling();

    let summary = run_task_graph(vec![&task], &fixture.ctx, &fixture.log, None, None).unwrap();

    assert_eq!(
        summary.outcome(&task.task_id()),
        Some(TaskOutcome::Satisfied)
    );
    assert!(!summary.was_interrupted());
    assert_eq!(summary.failure_count(), 0);
    let error = finish_run(&fixture.ctx, &fixture.log, &summary).unwrap_err();
    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert!(
        fixture
            .log
            .captured_lines()
            .iter()
            .any(|line| line.starts_with("Interrupted · ")),
        "late cancellation must not claim success"
    );
}

#[test]
fn completion_uses_scheduler_outcomes_even_without_presentation_records() {
    let (ctx, log) = execution_context(false);
    let mut summary = crate::engine::scheduler::ExecutionSummary::default();
    summary.record(numeric_task_id(1), "internal task", TaskOutcome::Cancelled);

    assert!(
        !log.has_interrupted_tasks(),
        "the logger has no task records"
    );
    let error = finish_run(&ctx, &log, &summary).unwrap_err();
    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert!(
        log.captured_lines()
            .iter()
            .any(|line| line.starts_with("Interrupted · "))
    );
}

#[test]
fn hidden_failure_precedes_cancellation_in_both_result_and_totals() {
    let (ctx, log) = execution_context(false);
    ctx.cancellation_token().cancel();
    let mut summary = crate::engine::scheduler::ExecutionSummary::default();
    summary.record(numeric_task_id(1), "internal failure", TaskOutcome::Failed);
    summary.add_failures(1);
    let error = finish_run(&ctx, &log, &summary).unwrap_err();
    assert!(error.is::<TaskFailures>());
    assert!(
        log.captured_lines()
            .iter()
            .any(|line| line.starts_with("Failed · "))
    );
    assert!(
        !log.captured_lines()
            .iter()
            .any(|line| line.contains("No applicable tasks") || line.contains("unchanged"))
    );
}

#[test]
fn run_accepts_an_empty_task_list() {
    let fixture = ExecutionFixture::with_parallel(false);
    fixture
        .run(&[])
        .expect("an empty graph is a successful no-op");
}

#[test]
fn cancelled_empty_plan_is_interrupted() {
    let fixture = ExecutionFixture::with_parallel(false);
    fixture.ctx.cancellation_token().cancel();

    let error = fixture.run(&[]).unwrap_err();

    assert!(error.is::<CommandInterrupted>(), "{error:#}");
}
