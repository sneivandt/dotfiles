//! Application task execution policy.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::engine::{Context, Task, TaskId};
use crate::infra::logging::{Logger, OutputExt as _};

use super::error::TaskFailures;
mod elevation;

use elevation::ElevationBroker;
#[cfg(test)]
use elevation::{blocked_dependents, build_elevated_child_args};

type LateTaskProvider<'a> = Box<dyn FnOnce() -> Vec<Box<dyn Task>> + 'a>;

/// A complete application execution plan.
///
/// The plan separates task-set discovery from execution policy. A simple plan
/// runs one graph; a phased plan runs the dependency closure ending at a
/// discovery boundary, obtains late tasks from refreshed state, then schedules
/// them with the remaining static tasks.
pub(crate) struct ExecutionPlan<'a> {
    tasks: Vec<&'a dyn Task>,
    late: Option<LateTaskPlan<'a>>,
}

struct LateTaskPlan<'a> {
    boundary: TaskId,
    provider: LateTaskProvider<'a>,
}

impl<'a> ExecutionPlan<'a> {
    /// Build a single-graph plan.
    pub(crate) fn single(tasks: impl IntoIterator<Item = &'a dyn Task>) -> Self {
        Self {
            tasks: tasks.into_iter().collect(),
            late: None,
        }
    }

    /// Build a plan that discovers additional tasks after `boundary`.
    pub(crate) fn with_late_tasks(
        tasks: impl IntoIterator<Item = &'a dyn Task>,
        boundary: TaskId,
        provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
    ) -> Self {
        Self {
            tasks: tasks.into_iter().collect(),
            late: Some(LateTaskPlan {
                boundary,
                provider: Box::new(provider),
            }),
        }
    }
}

/// Coordinates application execution phases around the generic task engine.
///
/// The engine owns graph validation and scheduling. This coordinator owns
/// application policy that spans graphs: visible progress totals, configuration
/// reload boundaries, late task discovery, elevation preparation, and final
/// run status.
#[derive(Debug)]
pub(crate) struct RunCoordinator<'a> {
    ctx: &'a Context,
    log: &'a Arc<Logger>,
}

impl<'a> RunCoordinator<'a> {
    /// Create a coordinator for one command run.
    pub(crate) const fn new(ctx: &'a Context, log: &'a Arc<Logger>) -> Self {
        Self { ctx, log }
    }

    /// Execute an application plan to completion.
    ///
    /// # Errors
    ///
    /// Returns an error if graph validation fails or one or more tasks fail.
    pub(crate) fn execute(&self, mut plan: ExecutionPlan<'_>) -> Result<()> {
        self.log
            .add_task_total(visible_count(plan.tasks.iter().copied()));

        if let Some(late) = plan.late.take() {
            self.execute_phased(plan.tasks, late)?;
        } else {
            run_task_graph(&mut plan.tasks, self.ctx, self.log)?;
        }

        finish_run(self.log)
    }

    fn execute_phased(&self, tasks: Vec<&dyn Task>, late: LateTaskPlan<'_>) -> Result<()> {
        let boundary_closure = dependency_closure(&tasks, late.boundary);

        if boundary_closure.is_empty() {
            let late_tasks = (late.provider)();
            self.log
                .add_task_total(visible_count(late_tasks.iter().map(Box::as_ref)));
            let mut all_tasks = tasks;
            all_tasks.extend(late_tasks.iter().map(Box::as_ref));
            run_task_graph(&mut all_tasks, self.ctx, self.log)?;
        } else {
            let mut prefix = tasks
                .iter()
                .copied()
                .filter(|task| boundary_closure.contains(&task.task_id()))
                .collect::<Vec<_>>();
            run_task_graph(&mut prefix, self.ctx, self.log)?;

            if self.log.failure_count() == 0 && !self.ctx.is_cancelled() {
                let late_tasks = (late.provider)();
                self.log
                    .add_task_total(visible_count(late_tasks.iter().map(Box::as_ref)));
                let mut remaining = tasks
                    .iter()
                    .copied()
                    .filter(|task| !boundary_closure.contains(&task.task_id()))
                    .collect::<Vec<_>>();
                remaining.extend(late_tasks.iter().map(Box::as_ref));
                run_task_graph(&mut remaining, self.ctx, self.log)?;
            }
        }

        Ok(())
    }
}

/// Execute a dependency-driven task graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
#[cfg(test)]
pub(crate) fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    RunCoordinator::new(ctx, log).execute(ExecutionPlan::single(tasks))
}

/// Execute tasks and inject additional tasks after a dependency boundary.
///
/// When `boundary` is present, its complete dependency closure runs first. The
/// provider then observes any state refreshed by that closure, and its tasks
/// join the remaining static tasks in a second dependency graph. If the
/// boundary was filtered out, the provider runs before the single graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
#[cfg(test)]
pub(crate) fn run_tasks_to_completion_with_late_tasks<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    boundary: TaskId,
    provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
) -> Result<()> {
    RunCoordinator::new(ctx, log).execute(ExecutionPlan::with_late_tasks(tasks, boundary, provider))
}

/// Count the tasks a run will report on, excluding internal ones.
///
/// The progress denominator and the run summary must agree, and the summary
/// only accounts for visible tasks.
fn visible_count<'a>(tasks: impl IntoIterator<Item = &'a dyn Task>) -> usize {
    tasks
        .into_iter()
        .filter(|task| task.visibility().is_visible())
        .count()
}

fn dependency_closure(tasks: &[&dyn Task], boundary: TaskId) -> HashSet<TaskId> {
    let by_id = tasks
        .iter()
        .map(|task| (task.task_id(), *task))
        .collect::<HashMap<_, _>>();
    if !by_id.contains_key(&boundary) {
        return HashSet::new();
    }

    let mut closure = HashSet::from([boundary]);
    let mut pending = vec![boundary];
    while let Some(id) = pending.pop() {
        if let Some(task) = by_id.get(&id) {
            for dependency in task.dependencies() {
                if by_id.contains_key(dependency) && closure.insert(*dependency) {
                    pending.push(*dependency);
                }
            }
        }
    }
    closure
}

fn run_task_graph(tasks: &mut Vec<&dyn Task>, ctx: &Context, log: &Arc<Logger>) -> Result<()> {
    if ctx.is_cancelled() || tasks.is_empty() {
        return Ok(());
    }

    ElevationBroker::new(ctx, log).prepare(tasks);

    if tasks.is_empty() {
        return Ok(());
    }

    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(tasks).map_err(|error| {
        let message = format!("{error} detected in task graph");
        log.error(&message);
        anyhow!(message)
    })?;
    if ctx.parallel() {
        crate::engine::scheduler::run_tasks_parallel(tasks, &graph, ctx, log);
    } else {
        crate::engine::scheduler::run_tasks_sequential(tasks, &graph, ctx, log);
    }
    Ok(())
}

fn finish_run(log: &Arc<Logger>) -> Result<()> {
    log.print_summary();
    let count = log.failure_count();
    if count > 0 {
        return Err(TaskFailures::new(count).into());
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use std::sync::Mutex;

    use proptest::prelude::*;

    use crate::engine::{TaskMeta, TaskResult};
    use crate::test_helpers::{empty_config, make_static_context};

    use super::*;

    /// Shared ordered record of which tasks ran.
    type Trace = Arc<Mutex<Vec<String>>>;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn elevated_child_args_scope_the_run_to_the_elevating_selectors() {
        let built = build_elevated_child_args(&args(&["install"]), &["developer-mode", "symlinks"]);

        assert_eq!(
            built,
            args(&[
                "install",
                "--only",
                "developer-mode,symlinks",
                "--no-parallel",
                "--elevated-child",
            ]),
        );
    }

    #[test]
    fn elevated_child_args_preserve_configuration_flags() {
        let built = build_elevated_child_args(
            &args(&[
                "install",
                "--profile",
                "desktop",
                "--root",
                "C:\\repo",
                "--overlay",
                "C:\\overlay",
            ]),
            &["developer-mode"],
        );

        assert_eq!(
            &built[..7],
            &args(&[
                "install",
                "--profile",
                "desktop",
                "--root",
                "C:\\repo",
                "--overlay",
                "C:\\overlay",
            ])[..]
        );
        assert!(built.contains(&"--only".to_string()));
    }

    #[test]
    fn elevated_child_args_drop_inherited_task_filters() {
        for filters in [
            args(&["install", "--only", "packages", "--skip", "registry"]),
            args(&["install", "--only=packages", "--skip=registry"]),
        ] {
            let built = build_elevated_child_args(&filters, &["developer-mode"]);

            assert_eq!(built.iter().filter(|arg| *arg == "--only").count(), 1);
            assert!(!built.iter().any(|arg| arg.contains("packages")));
            assert!(!built.iter().any(|arg| arg.contains("registry")));
        }
    }

    #[test]
    fn elevated_child_args_do_not_duplicate_repeated_flags() {
        let built = build_elevated_child_args(
            &args(&["install", "--no-parallel", "--elevated-child"]),
            &["developer-mode"],
        );

        assert_eq!(
            built.iter().filter(|arg| *arg == "--no-parallel").count(),
            1
        );
        assert_eq!(
            built
                .iter()
                .filter(|arg| *arg == "--elevated-child")
                .count(),
            1
        );
    }

    #[test]
    fn blocked_dependents_cascades_to_transitive_dependents() {
        // Mirrors the real Windows shape: symlinks cannot run unelevated, and
        // APM declares a dependency on it in the catalog.
        let trace = trace();
        let root = ProbeTask::new("Home symlinks", 1, &trace);
        let direct = ProbeTask::new("APM packages", 2, &trace).depends_on(&[1]);
        let indirect = ProbeTask::new("APM updates", 3, &trace).depends_on(&[2]);
        let unrelated = ProbeTask::new("Registry settings", 4, &trace);
        let tasks: Vec<&dyn Task> = vec![&root, &direct, &indirect, &unrelated];
        let roots = HashMap::from([(TaskId::Dynamic(1), "Home symlinks")]);

        let blocked = blocked_dependents(&tasks, &roots);

        // The reason names the root so the message stays actionable, rather
        // than pointing at the intermediate hop.
        assert_eq!(blocked.get(&TaskId::Dynamic(2)), Some(&"Home symlinks"));
        assert_eq!(blocked.get(&TaskId::Dynamic(3)), Some(&"Home symlinks"));
        assert!(!blocked.contains_key(&TaskId::Dynamic(4)));
        assert!(!blocked.contains_key(&TaskId::Dynamic(1)));
    }

    #[test]
    fn blocked_dependents_is_independent_of_slice_order() {
        // The task slice is not topologically sorted, so a dependent can be
        // visited before the dependency that blocks it.
        let trace = trace();
        let root = ProbeTask::new("Home symlinks", 1, &trace);
        let direct = ProbeTask::new("APM packages", 2, &trace).depends_on(&[1]);
        let indirect = ProbeTask::new("APM updates", 3, &trace).depends_on(&[2]);
        let tasks: Vec<&dyn Task> = vec![&indirect, &direct, &root];
        let roots = HashMap::from([(TaskId::Dynamic(1), "Home symlinks")]);

        let blocked = blocked_dependents(&tasks, &roots);

        assert_eq!(blocked.get(&TaskId::Dynamic(3)), Some(&"Home symlinks"));
        assert_eq!(blocked.len(), 2);
    }

    #[test]
    fn blocked_dependents_ignores_dependencies_absent_from_the_slice() {
        let trace = trace();
        let orphan = ProbeTask::new("Registry settings", 2, &trace).depends_on(&[99]);
        let tasks: Vec<&dyn Task> = vec![&orphan];
        let roots = HashMap::from([(TaskId::Dynamic(1), "Home symlinks")]);

        assert!(blocked_dependents(&tasks, &roots).is_empty());
    }

    fn trace() -> Trace {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn entries(trace: &Trace) -> Vec<String> {
        trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A task with a runtime-supplied identity, dependency list, and outcome.
    ///
    /// [`TaskId::Dynamic`] lets a test build an arbitrary graph shape, which
    /// type-derived ids cannot express.
    struct ProbeTask {
        name: String,
        id: TaskId,
        dependencies: Vec<TaskId>,
        trace: Trace,
        fails: bool,
    }

    impl ProbeTask {
        fn new(name: &str, id: u64, trace: &Trace) -> Self {
            Self {
                name: name.to_string(),
                id: TaskId::Dynamic(id),
                dependencies: Vec::new(),
                trace: Arc::clone(trace),
                fails: false,
            }
        }

        fn depends_on(mut self, ids: &[u64]) -> Self {
            self.dependencies = ids.iter().copied().map(TaskId::Dynamic).collect();
            self
        }

        const fn failing(mut self) -> Self {
            self.fails = true;
            self
        }
    }

    impl Task for ProbeTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new(&self.name)
        }

        fn task_id(&self) -> TaskId {
            self.id
        }

        fn dependencies(&self) -> &[TaskId] {
            &self.dependencies
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.trace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(self.name.clone());
            if self.fails {
                anyhow::bail!("{} failed", self.name);
            }
            Ok(TaskResult::Ok)
        }
    }

    fn sequential_context() -> (Context, Arc<Logger>) {
        let (ctx, log) = make_static_context(empty_config(std::path::PathBuf::from("/tmp")));
        // Sequential execution keeps the recorded trace deterministic, so
        // ordering assertions describe dependency edges rather than thread
        // scheduling luck.
        (ctx.with_parallel(false), log)
    }

    fn as_dyn(tasks: &[ProbeTask]) -> Vec<&dyn Task> {
        tasks.iter().map(|task| -> &dyn Task { task }).collect()
    }

    // ── dependency_closure ────────────────────────────────

    #[test]
    fn closure_is_empty_when_boundary_is_not_present() {
        let trace = trace();
        let tasks = vec![ProbeTask::new("a", 1, &trace)];
        assert!(
            dependency_closure(&as_dyn(&tasks), TaskId::Dynamic(99)).is_empty(),
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

        let closure = dependency_closure(&as_dyn(&tasks), TaskId::Dynamic(3));

        assert_eq!(
            closure.len(),
            3,
            "closure should hold root, middle, boundary"
        );
        for id in [1, 2, 3] {
            assert!(
                closure.contains(&TaskId::Dynamic(id)),
                "closure should contain dynamic id {id}"
            );
        }
        assert!(
            !closure.contains(&TaskId::Dynamic(4)),
            "a dependent of the boundary must not be pulled into the prefix"
        );
    }

    #[test]
    fn closure_ignores_dependencies_absent_from_the_task_list() {
        let trace = trace();
        // Depending on a filtered-out task is normal: `--only` can remove a
        // dependency while keeping its dependent.
        let tasks = vec![ProbeTask::new("boundary", 3, &trace).depends_on(&[42])];

        let closure = dependency_closure(&as_dyn(&tasks), TaskId::Dynamic(3));

        assert_eq!(
            closure,
            HashSet::from([TaskId::Dynamic(3)]),
            "an unmatched dependency id must not enter the closure"
        );
    }

    #[test]
    fn closure_terminates_on_self_referential_dependencies() {
        let trace = trace();
        // Graph validation rejects cycles later; the closure walk must still
        // terminate rather than spin.
        let tasks = vec![ProbeTask::new("boundary", 1, &trace).depends_on(&[1])];

        let closure = dependency_closure(&as_dyn(&tasks), TaskId::Dynamic(1));

        assert_eq!(closure, HashSet::from([TaskId::Dynamic(1)]));
    }

    proptest! {
        #[test]
        fn graph_closures_and_blocked_cascades_reach_fixed_points(
            (size, edges) in (1_usize..8).prop_flat_map(|size| {
                (
                    Just(size),
                    proptest::collection::vec(any::<bool>(), size.saturating_mul(size)),
                )
            })
        ) {
            let trace = trace();
            let tasks = (0..size)
                .map(|row| {
                    let id = u64::try_from(row).expect("small graph index").saturating_add(1);
                    let dependencies = (0..size)
                        .filter(|column| edges[row.saturating_mul(size).saturating_add(*column)])
                        .map(|column| {
                            u64::try_from(column)
                                .expect("small graph index")
                                .saturating_add(1)
                        })
                        .collect::<Vec<_>>();
                    ProbeTask::new(&format!("task-{id}"), id, &trace).depends_on(&dependencies)
                })
                .collect::<Vec<_>>();
            let task_refs = as_dyn(&tasks);

            let mut expected_dependencies = vec![false; size];
            expected_dependencies[0] = true;
            loop {
                let mut changed = false;
                for row in 0..size {
                    if !expected_dependencies[row] {
                        continue;
                    }
                    for column in 0..size {
                        if edges[row.saturating_mul(size).saturating_add(column)]
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
            let closure = dependency_closure(&task_refs, TaskId::Dynamic(1));
            for (index, expected) in expected_dependencies.iter().copied().enumerate() {
                let id = TaskId::Dynamic(
                    u64::try_from(index)
                        .expect("small graph index")
                        .saturating_add(1),
                );
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
                        edges[row.saturating_mul(size).saturating_add(column)]
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
            let roots = HashMap::from([(TaskId::Dynamic(1), "task-1")]);
            let blocked = blocked_dependents(&task_refs, &roots);
            for (index, expected) in expected_blocked.iter().copied().enumerate().skip(1) {
                let id = TaskId::Dynamic(
                    u64::try_from(index)
                        .expect("small graph index")
                        .saturating_add(1),
                );
                prop_assert_eq!(blocked.contains_key(&id), expected);
            }
        }
    }

    // ── run_tasks_to_completion ───────────────────────────

    #[test]
    fn run_executes_tasks_in_dependency_order() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("second", 2, &trace).depends_on(&[1]),
            ProbeTask::new("first", 1, &trace),
        ];
        let (ctx, log) = sequential_context();

        run_tasks_to_completion(as_dyn(&tasks), &ctx, &log).expect("run should succeed");

        assert_eq!(
            entries(&trace),
            vec!["first".to_string(), "second".to_string()],
            "list order is not execution order; dependency edges are"
        );
    }

    #[test]
    fn run_reports_failure_count_when_a_task_fails() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("ok", 1, &trace),
            ProbeTask::new("bad", 2, &trace).failing(),
        ];
        let (ctx, log) = sequential_context();

        let error = run_tasks_to_completion(as_dyn(&tasks), &ctx, &log)
            .expect_err("a failing task must fail the run");

        assert!(
            error.downcast_ref::<TaskFailures>().is_some(),
            "failures should surface as TaskFailures, got: {error:#}"
        );
    }

    #[test]
    fn run_blocks_dependents_of_a_failed_task() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("bad", 1, &trace).failing(),
            ProbeTask::new("dependent", 2, &trace).depends_on(&[1]),
        ];
        let (ctx, log) = sequential_context();

        let _error = run_tasks_to_completion(as_dyn(&tasks), &ctx, &log).unwrap_err();

        assert_eq!(
            entries(&trace),
            vec!["bad".to_string()],
            "a task whose prerequisite failed must not run"
        );
    }

    #[test]
    fn run_rejects_a_cyclic_graph_before_executing_anything() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("a", 1, &trace).depends_on(&[2]),
            ProbeTask::new("b", 2, &trace).depends_on(&[1]),
        ];
        let (ctx, log) = sequential_context();

        let error = run_tasks_to_completion(as_dyn(&tasks), &ctx, &log)
            .expect_err("a cycle must be rejected");

        assert!(
            error.to_string().contains("task graph"),
            "error should name the task graph, got: {error:#}"
        );
        assert!(
            entries(&trace).is_empty(),
            "no task may run when graph validation fails"
        );
    }

    #[test]
    fn run_is_a_no_op_when_already_cancelled() {
        let trace = trace();
        let tasks = vec![ProbeTask::new("a", 1, &trace)];
        let (ctx, log) = sequential_context();
        ctx.cancellation_token().cancel();

        run_tasks_to_completion(as_dyn(&tasks), &ctx, &log)
            .expect("cancellation is not itself a failure");

        assert!(
            entries(&trace).is_empty(),
            "cancellation before dispatch must skip every task"
        );
    }

    #[test]
    fn run_accepts_an_empty_task_list() {
        let (ctx, log) = sequential_context();
        run_tasks_to_completion(Vec::<&dyn Task>::new(), &ctx, &log)
            .expect("an empty graph is a successful no-op");
    }

    // ── run_tasks_to_completion_with_late_tasks ───────────

    #[test]
    fn late_tasks_run_after_the_boundary_closure() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("before-boundary", 1, &trace),
            ProbeTask::new("boundary", 2, &trace).depends_on(&[1]),
            ProbeTask::new("after-boundary", 3, &trace).depends_on(&[2]),
        ];
        let (ctx, log) = sequential_context();
        let late_trace = Arc::clone(&trace);

        run_tasks_to_completion_with_late_tasks(
            as_dyn(&tasks),
            &ctx,
            &log,
            TaskId::Dynamic(2),
            move || -> Vec<Box<dyn Task>> {
                vec![Box::new(ProbeTask::new("late", 9, &late_trace))]
            },
        )
        .expect("run should succeed");

        let order = entries(&trace);
        let boundary = order.iter().position(|n| n == "boundary").unwrap();
        let late = order.iter().position(|n| n == "late").unwrap();
        assert!(
            boundary < late,
            "late tasks must be built only after the boundary closure completes: {order:?}"
        );
        assert!(
            order.contains(&"after-boundary".to_string()),
            "tasks outside the closure must still run: {order:?}"
        );
    }

    #[test]
    fn late_task_provider_observes_boundary_side_effects() {
        // This is the whole point of the boundary: the provider must be able to
        // read state that the closure refreshed (config reload rebuilding
        // dynamic overlay tasks).
        let trace = trace();
        let tasks = vec![ProbeTask::new("boundary", 1, &trace)];
        let (ctx, log) = sequential_context();
        let observed = Arc::new(Mutex::new(Vec::<String>::new()));

        let provider_trace = Arc::clone(&trace);
        let provider_observed = Arc::clone(&observed);
        run_tasks_to_completion_with_late_tasks(
            as_dyn(&tasks),
            &ctx,
            &log,
            TaskId::Dynamic(1),
            move || {
                *provider_observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = entries(&provider_trace);
                Vec::new()
            },
        )
        .expect("run should succeed");

        assert_eq!(
            *observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec!["boundary".to_string()],
            "the provider must see the boundary's effects"
        );
    }

    #[test]
    fn late_tasks_run_before_the_graph_when_boundary_is_filtered_out() {
        let trace = trace();
        let tasks = vec![ProbeTask::new("static", 1, &trace)];
        let (ctx, log) = sequential_context();
        let late_trace = Arc::clone(&trace);

        run_tasks_to_completion_with_late_tasks(
            as_dyn(&tasks),
            &ctx,
            &log,
            TaskId::Dynamic(404),
            move || -> Vec<Box<dyn Task>> {
                vec![Box::new(ProbeTask::new("late", 9, &late_trace))]
            },
        )
        .expect("run should succeed");

        let order = entries(&trace);
        assert!(
            order.contains(&"late".to_string()) && order.contains(&"static".to_string()),
            "a missing boundary should still run both static and late tasks: {order:?}"
        );
    }

    #[test]
    fn late_tasks_are_skipped_when_the_boundary_closure_fails() {
        let trace = trace();
        let tasks = vec![
            ProbeTask::new("boundary", 1, &trace).failing(),
            ProbeTask::new("after-boundary", 2, &trace).depends_on(&[1]),
        ];
        let (ctx, log) = sequential_context();
        let provider_called = Arc::new(Mutex::new(false));

        let flag = Arc::clone(&provider_called);
        let error = run_tasks_to_completion_with_late_tasks(
            as_dyn(&tasks),
            &ctx,
            &log,
            TaskId::Dynamic(1),
            move || {
                *flag
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Vec::new()
            },
        )
        .expect_err("a failed boundary must fail the run");

        assert!(error.downcast_ref::<TaskFailures>().is_some());
        assert!(
            !*provider_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            "late tasks must not be built from state a failed boundary left behind"
        );
        assert_eq!(
            entries(&trace),
            vec!["boundary".to_string()],
            "remaining tasks must not run after the boundary fails"
        );
    }

    #[test]
    fn late_tasks_are_skipped_when_cancelled_during_the_boundary_closure() {
        let trace = trace();
        let tasks = vec![ProbeTask::new("boundary", 1, &trace)];
        let (ctx, log) = sequential_context();
        let provider_called = Arc::new(Mutex::new(false));

        // Cancel after the closure has been scheduled but before the provider
        // would be consulted.
        ctx.cancellation_token().cancel();

        let flag = Arc::clone(&provider_called);
        run_tasks_to_completion_with_late_tasks(
            as_dyn(&tasks),
            &ctx,
            &log,
            TaskId::Dynamic(1),
            move || {
                *flag
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Vec::new()
            },
        )
        .expect("cancellation is not itself a failure");

        assert!(
            !*provider_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            "a cancelled run must not build late tasks"
        );
    }
}
