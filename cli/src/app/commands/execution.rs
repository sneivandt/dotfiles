//! Application task execution policy.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::engine::{Context, Task, TaskId, TaskVisibility};
use crate::infra::logging::{ActionCounts, Logger};

use super::error::TaskFailures;
use crate::infra::logging::OutputExt as _;

#[cfg(unix)]
fn prime_sudo(ctx: &Context, log: &Arc<Logger>, task_names: &[&str]) -> bool {
    if !crate::infra::elevation::sudo_available(ctx.executor()) {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return false;
    }
    log.debug("priming sudo credential cache");

    if crate::infra::elevation::sudo_credentials_cached() {
        log.debug("sudo credentials already cached");
        return true;
    }

    log.separate_from_startup();
    log.always(format!("sudo is required for: {}", task_names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match crate::infra::elevation::prime_sudo_credentials() {
        Ok(true) => true,
        Ok(false) => {
            log.separate_from_startup();
            log.error("sudo credential priming failed");
            false
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to run sudo: {error:#}"));
            false
        }
    }
}

#[cfg(not(unix))]
const fn prime_sudo(_ctx: &Context, _log: &Arc<Logger>, _task_names: &[&str]) -> bool {
    true
}

/// Execute a dependency-driven task graph.
///
/// # Errors
///
/// Returns an error if graph validation fails or one or more tasks fail.
pub(crate) fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
) -> Result<()> {
    let mut tasks = tasks.into_iter().collect::<Vec<_>>();
    log.add_task_total(visible_count(tasks.iter().copied()));
    run_task_graph(&mut tasks, ctx, log)?;
    finish_run(log)
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
pub(crate) fn run_tasks_to_completion_with_late_tasks<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Arc<Logger>,
    boundary: TaskId,
    provider: impl FnOnce() -> Vec<Box<dyn Task>> + 'a,
) -> Result<()> {
    let tasks = tasks.into_iter().collect::<Vec<_>>();
    // Seed the progress denominator with every statically known task so it does
    // not visibly jump when the run is split across two dependency graphs.
    log.add_task_total(visible_count(tasks.iter().copied()));
    let boundary_closure = dependency_closure(&tasks, boundary);

    if boundary_closure.is_empty() {
        let late_tasks = provider();
        log.add_task_total(visible_count(late_tasks.iter().map(Box::as_ref)));
        let mut all_tasks = tasks;
        all_tasks.extend(late_tasks.iter().map(Box::as_ref));
        run_task_graph(&mut all_tasks, ctx, log)?;
    } else {
        let mut prefix = tasks
            .iter()
            .copied()
            .filter(|task| boundary_closure.contains(&task.task_id()))
            .collect::<Vec<_>>();
        run_task_graph(&mut prefix, ctx, log)?;

        if log.failure_count() == 0 && !ctx.is_cancelled() {
            let late_tasks = provider();
            log.add_task_total(visible_count(late_tasks.iter().map(Box::as_ref)));
            let mut remaining = tasks
                .iter()
                .copied()
                .filter(|task| !boundary_closure.contains(&task.task_id()))
                .collect::<Vec<_>>();
            remaining.extend(late_tasks.iter().map(Box::as_ref));
            run_task_graph(&mut remaining, ctx, log)?;
        }
    }

    finish_run(log)
}

/// Count the tasks a run will report on, excluding internal ones.
///
/// The progress denominator and the run summary must agree, and the summary
/// only accounts for visible tasks.
fn visible_count<'a>(tasks: impl IntoIterator<Item = &'a dyn Task>) -> usize {
    tasks
        .into_iter()
        .filter(|task| task.visibility() == TaskVisibility::Visible)
        .count()
}

fn dependency_closure(tasks: &[&dyn Task], boundary: TaskId) -> HashSet<TaskId> {
    let by_id = tasks
        .iter()
        .map(|task| (task.task_id(), *task))
        .collect::<std::collections::HashMap<_, _>>();
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

    let sudo_task_names: Vec<&str> = if ctx.parallel() && !ctx.dry_run() && tasks.len() > 1 {
        tasks
            .iter()
            .filter(|task| task.requires_elevation(ctx))
            .map(|task| task.name())
            .collect()
    } else {
        Vec::new()
    };
    if !sudo_task_names.is_empty() && !prime_sudo(ctx, log, &sudo_task_names) {
        let reason = "sudo credentials unavailable";
        tasks.retain(|task| {
            if task.requires_elevation(ctx) {
                let span = tracing::info_span!("task", name = task.name());
                let _enter = span.enter();
                log.debug(reason);
                log.record_task_with_metadata(
                    task.name(),
                    crate::infra::logging::TaskStatus::Skipped,
                    Some(reason),
                    ActionCounts::default(),
                    task.visibility() == TaskVisibility::Visible,
                );
                log.mark_task_completed(task.name());
                log.emit_task_result_and_redraw(task.name());
                false
            } else {
                true
            }
        });
    }

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

    use crate::engine::TaskResult;
    use crate::test_helpers::{empty_config, make_static_context};

    use super::*;

    /// Shared ordered record of which tasks ran.
    type Trace = Arc<Mutex<Vec<String>>>;

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
        fn name(&self) -> &str {
            &self.name
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
