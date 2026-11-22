use super::*;

// ── run_tasks_to_completion_with_restart ──────────────

#[test]
fn restart_runs_after_the_boundary_closure_and_stops_the_parent() {
    let fixture = ExecutionFixture::with_parallel(false);
    let before = fixture.task("before-boundary", 1);
    let boundary = fixture.task("boundary", 2).depends_on(&[1]);
    let after = fixture.task("after-boundary", 3).depends_on(&[2]);
    let restarted = std::cell::Cell::new(false);

    fixture
        .run_with_restart(
            &[&before, &boundary, &after],
            boundary.task_id(),
            || true,
            || restarted.set(true),
        )
        .expect("restart handoff should succeed");

    assert!(restarted.get());
    assert_eq!(
        fixture.entries(),
        ["before-boundary", "boundary"],
        "the parent must not execute work after handing off to the child"
    );
}

#[test]
fn unsatisfied_restart_condition_runs_the_remaining_graph() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("boundary", 1);
    let remaining = fixture.task("remaining", 2).depends_on(&[1]);

    fixture
        .run_with_restart(
            &[&boundary, &remaining],
            boundary.task_id(),
            || false,
            || panic!("restart action must not run"),
        )
        .expect("run should continue without a restart");

    assert_eq!(fixture.entries(), ["boundary", "remaining"]);
}

#[test]
fn missing_boundary_falls_back_to_one_graph_without_restart() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let task = fixture.task("static", 1);

        fixture
            .run_with_restart(
                &[&task],
                numeric_task_id(404),
                || true,
                || panic!("a filtered boundary must not trigger restart"),
            )
            .expect("missing boundary should use a single graph");

        assert_eq!(fixture.entries(), ["static"], "parallel={parallel}");
    }
}

#[test]
fn failed_boundary_suppresses_restart() {
    for parallel in [false, true] {
        for failure in [ProbeFailure::Error, ProbeFailure::Result] {
            let fixture = ExecutionFixture::with_parallel(parallel);
            let mut boundary = fixture.task("boundary", 1);
            boundary.failure = Some(failure);
            let remaining = fixture.task("remaining", 2).depends_on(&[1]);

            let error = fixture
                .run_with_restart(
                    &[&boundary, &remaining],
                    boundary.task_id(),
                    || true,
                    || panic!("a failed boundary must not trigger restart"),
                )
                .expect_err("boundary failure must fail the run");

            assert!(
                error.downcast_ref::<TaskFailures>().is_some(),
                "parallel={parallel}, failure={failure:?}: {error:#}"
            );
            assert_eq!(
                fixture.entries(),
                ["boundary"],
                "parallel={parallel}, failure={failure:?}"
            );
        }
    }
}

#[test]
fn failed_boundary_stops_independent_tasks_when_it_changed_restart_inputs() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("partially-updated-boundary", 1).failing();
    let independent = fixture.task("independent", 2);

    let error = fixture
        .run_with_restart(
            &[&boundary, &independent],
            boundary.task_id(),
            || true,
            || panic!("a failed update must be reported before a re-exec is attempted"),
        )
        .expect_err("the boundary failure must fail the run");

    assert!(error.downcast_ref::<TaskFailures>().is_some());
    assert_eq!(
        fixture.entries(),
        ["partially-updated-boundary"],
        "remaining tasks must not use inputs loaded before the partial update"
    );
}

#[test]
fn cancellation_suppresses_restart() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("boundary", 1);
    fixture.ctx.cancellation_token().cancel();

    let error = fixture
        .run_with_restart(
            &[&boundary],
            boundary.task_id(),
            || panic!("a cancelled run must not inspect restart inputs"),
            || panic!("a cancelled run must not trigger restart"),
        )
        .expect_err("a cancelled run must be interrupted");

    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert!(fixture.entries().is_empty());
}

#[test]
fn typed_interruption_stops_the_next_phase_and_suppresses_restart() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("boundary", 1).interrupted();
    let independent = fixture.task("independent", 2);

    let error = fixture
        .run_with_restart(
            &[&boundary, &independent],
            boundary.task_id(),
            || panic!("an interrupted phase must not inspect restart inputs"),
            || panic!("an interrupted phase must not restart"),
        )
        .unwrap_err();

    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert!(!fixture.ctx.is_cancelled());
    assert_eq!(fixture.entries(), ["boundary"]);
}

#[test]
fn cancellation_after_boundary_completion_suppresses_restart() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("boundary", 1).cancelling();
    let independent = fixture.task("independent", 2);

    let error = fixture
        .run_with_restart(
            &[&boundary, &independent],
            boundary.task_id(),
            || panic!("a cancelled phase must not inspect restart inputs"),
            || panic!("a completed boundary must not restart a cancelled command"),
        )
        .unwrap_err();

    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert_eq!(fixture.entries(), ["boundary"]);
}

#[test]
fn cancellation_during_restart_check_suppresses_the_action() {
    let fixture = ExecutionFixture::with_parallel(false);
    let boundary = fixture.task("boundary", 1);

    let error = fixture
        .run_with_restart(
            &[&boundary],
            boundary.task_id(),
            || {
                fixture.ctx.cancellation_token().cancel();
                true
            },
            || panic!("a late cancellation must still suppress restart"),
        )
        .unwrap_err();

    assert!(error.is::<CommandInterrupted>(), "{error:#}");
    assert_eq!(fixture.entries(), ["boundary"]);
}

#[test]
fn prepared_phase_resolves_conditional_waits_again_for_each_run() {
    struct Consumer {
        trace: Trace,
        ready: bool,
    }
    struct Provider(Trace);
    impl Task for Provider {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("provider")
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.0.lock().unwrap().push("provider".into());
            Ok(TaskResult::Ok)
        }
    }
    impl Task for Consumer {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("consumer")
        }
        fn ordering_requirements(&self) -> Vec<crate::engine::Requirement<'_>> {
            vec![crate::engine::Requirement::new("consumer-input", |_, _| {
                Ok(if self.ready {
                    crate::engine::RequirementState::Satisfied(())
                } else {
                    crate::engine::RequirementState::unsatisfied("input missing")
                })
            })]
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.trace.lock().unwrap().push("consumer".into());
            Ok(TaskResult::Ok)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (ctx, log) = make_static_context(root.path().to_path_buf(), None);
    let ctx = ctx.with_parallel(false);
    for ready in [true, false] {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let provider = Provider(Arc::clone(&trace));
        let consumer = crate::engine::TaskWithExtraDeps::new(
            Box::new(Consumer {
                trace: Arc::clone(&trace),
                ready,
            }),
            &[],
            &[provider.task_id()],
        )
        .with_conditional_ordering(
            &[provider.task_id()],
            crate::engine::Requirement::new("provider-stable", |_, _| {
                Ok(crate::engine::RequirementState::Satisfied(()))
            }),
        );
        let tasks: [&dyn Task; 2] = [&consumer, &provider];
        run_tasks_to_completion(tasks, &ctx, &log).unwrap();
        let expected = if ready {
            vec!["consumer", "provider"]
        } else {
            vec!["provider", "consumer"]
        };
        assert_eq!(*trace.lock().unwrap(), expected);
    }
}
