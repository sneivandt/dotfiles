use super::*;

fn as_dyn(tasks: &[ProbeTask]) -> Vec<&dyn Task> {
    tasks.iter().map(|task| -> &dyn Task { task }).collect()
}

#[test]
fn conditional_readiness_only_probes_for_current_or_prior_providers() {
    use crate::engine::scheduler::ExecutionSummary;
    use crate::engine::{Requirement, RequirementState, TaskWithExtraDeps};

    struct Consumer(Arc<AtomicUsize>);

    impl Task for Consumer {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("consumer")
        }

        fn ordering_requirements(&self) -> Vec<Requirement<'_>> {
            vec![Requirement::new("ready", |_, _| {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(RequirementState::Satisfied(()))
            })]
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    for (selected, ran_earlier) in [(false, false), (true, false), (false, true)] {
        let fixture = ExecutionFixture::with_parallel(false);
        let provider = fixture.task("provider", 1);
        let providers = [numeric_task_id(99), provider.task_id()];
        let probes = Arc::new(AtomicUsize::new(0));
        let consumer =
            TaskWithExtraDeps::new(Box::new(Consumer(Arc::clone(&probes))), &[], &providers)
                .with_conditional_ordering(
                    &providers,
                    Requirement::new("stable", |_, _| Ok(RequirementState::Satisfied(()))),
                );
        let mut tasks: Vec<&dyn Task> = vec![&consumer];
        if selected {
            tasks.push(&provider);
        }
        let mut prior = ExecutionSummary::default();
        if ran_earlier {
            prior.record(provider.task_id(), provider.name(), TaskOutcome::Cancelled);
        }
        let summary = run_task_graph(tasks, &fixture.ctx, &fixture.log, Some(&prior)).unwrap();
        assert_eq!(
            probes.load(Ordering::SeqCst),
            usize::from(selected || ran_earlier)
        );
        assert_eq!(
            summary.outcome(&consumer.task_id()),
            Some(TaskOutcome::Satisfied)
        );
    }
}

#[test]
fn prepared_phase_rebuilds_after_elevation_without_reassessing() {
    for parallel in [false, true] {
        let fixture = ExecutionFixture::with_parallel(parallel);
        let ctx = fixture.ctx.with_non_interactive(true);
        let mut elevated = fixture.task("elevated", 1);
        elevated.requires_elevation = true;
        let tasks = [
            elevated,
            fixture.task("dependent", 4).depends_on(&[3]),
            fixture.task("independent", 3),
            fixture.task("blocked", 2).depends_on(&[1]),
        ];
        let summary = run_task_graph(as_dyn(&tasks), &ctx, &fixture.log, None).unwrap();

        assert_eq!(fixture.entries(), ["independent", "dependent"]);
        for (task, expected) in tasks.iter().zip([
            TaskOutcome::Unmet,
            TaskOutcome::Satisfied,
            TaskOutcome::Satisfied,
            TaskOutcome::Blocked,
        ]) {
            assert_eq!(summary.outcome(&task.task_id()), Some(expected));
            assert_eq!(task.assessments.load(Ordering::SeqCst), 1, "{}", task.name);
        }
    }
}

// ── dependency_closure ────────────────────────────────

#[test]
fn closure_is_empty_when_boundary_is_not_present() {
    let trace = trace();
    let tasks = vec![ProbeTask::new("a", 1, &trace)];
    assert!(
        dependency_closure(&as_dyn(&tasks), numeric_task_id(99))
            .expect("valid graph")
            .is_empty(),
        "a filtered-out boundary must produce an empty closure so the \
             caller falls back to a single graph"
    );
}

#[test]
fn closure_contains_boundary_and_transitive_dependencies() {
    let trace = trace();
    let tasks = vec![
        ProbeTask::new("root", 1, &trace),
        ProbeTask::new("middle", 2, &trace).depends_on(&[1]),
        ProbeTask::new("boundary", 3, &trace).depends_on(&[2]),
        ProbeTask::new("after", 4, &trace).depends_on(&[3]),
    ];

    let closure = dependency_closure(&as_dyn(&tasks), numeric_task_id(3)).expect("valid graph");

    assert_eq!(
        closure.len(),
        3,
        "closure should hold root, middle, boundary"
    );
    for id in [1, 2, 3] {
        assert!(
            closure.contains(&numeric_task_id(id)),
            "closure should contain dynamic id {id}"
        );
    }
    assert!(
        !closure.contains(&numeric_task_id(4)),
        "a dependent of the boundary must not be pulled into the prefix"
    );
}

#[test]
fn closure_ignores_dependencies_absent_from_the_task_list() {
    let trace = trace();
    // Depending on a filtered-out task is normal: `--only` can remove a
    // dependency while keeping its dependent.
    let tasks = vec![ProbeTask::new("boundary", 3, &trace).depends_on(&[42])];

    let closure = dependency_closure(&as_dyn(&tasks), numeric_task_id(3)).expect("valid graph");

    assert_eq!(
        closure,
        HashSet::from([numeric_task_id(3)]),
        "an unmatched dependency id must not enter the closure"
    );
}

#[test]
fn closure_rejects_self_referential_dependencies() {
    let trace = trace();
    let tasks = vec![ProbeTask::new("boundary", 1, &trace).depends_on(&[1])];

    assert!(dependency_closure(&as_dyn(&tasks), numeric_task_id(1)).is_err());
}

proptest! {
    #[test]
    fn graph_closures_and_blocked_cascades_reach_fixed_points(
        (size, edges) in (1_usize..8).prop_flat_map(|size| {
            (
                Just(size),
                proptest::collection::vec(any::<bool>(), size * size),
            )
        })
    ) {
        let trace = trace();
        let tasks = (0..size)
            .map(|row| {
                let id = u64::try_from(row).expect("small graph index") + 1;
                let dependencies = (0..size)
                    .filter(|column| {
                        *column < row
                            && edges[row * size + *column]
                    })
                    .map(|column| {
                        u64::try_from(column).expect("small graph index") + 1
                    })
                    .collect::<Vec<_>>();
                ProbeTask::new(&format!("task-{id}"), id, &trace).depends_on(&dependencies)
            })
            .collect::<Vec<_>>();
        let task_refs = as_dyn(&tasks);

        let mut expected_dependencies = vec![false; size];
        expected_dependencies[size - 1] = true;
        loop {
            let mut changed = false;
            for row in 0..size {
                if !expected_dependencies[row] {
                    continue;
                }
                for column in 0..size {
                    if column < row
                        && edges[row * size + column]
                        && !expected_dependencies[column]
                    {
                        expected_dependencies[column] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let boundary = numeric_task_id(size);
        let closure = dependency_closure(&task_refs, boundary)
            .expect("valid generated graph");
        for (index, expected) in expected_dependencies.iter().copied().enumerate() {
            let id = numeric_task_id(index + 1);
            prop_assert_eq!(closure.contains(&id), expected);
        }

        let mut expected_blocked = vec![false; size];
        expected_blocked[0] = true;
        loop {
            let mut changed = false;
            for row in 1..size {
                if expected_blocked[row] {
                    continue;
                }
                let blocked = (0..size).any(|column| {
                    column < row
                        && edges[row * size + column]
                        && expected_blocked[column]
                });
                if blocked {
                    expected_blocked[row] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let roots = HashMap::from([(numeric_task_id(1), "task-1")]);
        let blocked = blocked_dependents(&task_refs, &roots);
        for (index, expected) in expected_blocked.iter().copied().enumerate().skip(1) {
            let id = numeric_task_id(index + 1);
            prop_assert_eq!(blocked.contains_key(&id), expected);
        }
    }
}
