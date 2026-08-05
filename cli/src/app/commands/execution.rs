//! Application task execution policy.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::engine::{Context, Task, TaskId};
use crate::infra::logging::{ActionCounts, Logger};

use super::error::TaskFailures;
use crate::infra::logging::OutputExt as _;

/// Outcome of arranging privilege for the tasks that declared they need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElevationPlan {
    /// Privilege is available to this process; run the tasks normally.
    Ready,
    /// The tasks already ran elsewhere; drop them from this run's graph.
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "only the Windows broker delegates to a child")
    )]
    Delegated,
    /// Privilege could not be arranged; skip the tasks and continue.
    Unavailable { reason: &'static str },
}

/// Arrange privilege for `names`, or report that it is unavailable.
///
/// Unix keeps the existing behaviour: prime the `sudo` credential cache once so
/// parallel tasks do not interleave password prompts, and let sequential runs
/// prompt inline as they always have.
#[cfg(unix)]
fn prepare_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    _selectors: &[&str],
    task_count: usize,
) -> ElevationPlan {
    // A single task, or a sequential run, can prompt inline without garbling
    // output, so there is nothing to arrange up front.
    if !ctx.parallel() || task_count <= 1 {
        return ElevationPlan::Ready;
    }

    if !crate::infra::elevation::sudo_available(ctx.executor()) {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return ElevationPlan::Unavailable {
            reason: "sudo credentials unavailable",
        };
    }
    log.debug("priming sudo credential cache");

    if crate::infra::elevation::sudo_credentials_cached() {
        log.debug("sudo credentials already cached");
        return ElevationPlan::Ready;
    }

    log.separate_from_startup();
    log.always(format!("sudo is required for: {}", names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match crate::infra::elevation::prime_sudo_credentials() {
        Ok(true) => ElevationPlan::Ready,
        Ok(false) => {
            log.separate_from_startup();
            log.error("sudo credential priming failed");
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to run sudo: {error:#}"));
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        }
    }
}

/// Delegate the elevating tasks to a single short-lived elevated child run.
///
/// Windows has no per-command `sudo`, so the alternative to one scoped child is
/// elevating the whole run. The child is restricted to `selectors`, so only the
/// tasks that declared `needs_elevation` ever hold an administrator token; this
/// process keeps running unelevated in the user's own terminal.
#[cfg(windows)]
fn prepare_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    selectors: &[&str],
    _task_count: usize,
) -> ElevationPlan {
    use crate::infra::elevation::{ElevationOutcome, run_elevated_child};

    if selectors.is_empty() {
        return ElevationPlan::Ready;
    }

    // A UAC consent dialog is drawn on the interactive secure desktop. In CI or
    // any other headless session there is nobody to answer it, so requesting it
    // would at best fail and at worst stall the run until the command timeout.
    // Degrade to the same outcome as a declined prompt instead.
    if ctx.system().is_ci() || !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        log.warn(format!(
            "administrator access is required for: {}",
            names.join(", ")
        ));
        return ElevationPlan::Unavailable {
            reason: "elevation unavailable in a non-interactive session",
        };
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let child_args = build_elevated_child_args(&args, selectors);

    log.separate_from_startup();
    log.always(format!(
        "administrator access is required for: {}",
        names.join(", ")
    ));
    log.always("A UAC prompt will open; the rest of this run stays unelevated.");
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match run_elevated_child(ctx.executor(), &**log, &child_args) {
        Ok(ElevationOutcome::Completed) => {
            log.always("Elevated step finished.");
            ElevationPlan::Delegated
        }
        Ok(ElevationOutcome::Declined) => {
            log.separate_from_startup();
            log.warn("elevation declined; continuing without it");
            ElevationPlan::Unavailable {
                reason: "elevation declined",
            }
        }
        Ok(ElevationOutcome::Failed(code)) => {
            log.separate_from_startup();
            log.error(format!("elevated step failed (exit code {code})"));
            ElevationPlan::Unavailable {
                reason: "elevated step failed",
            }
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to request elevation: {error:#}"));
            ElevationPlan::Unavailable {
                reason: "elevation unavailable",
            }
        }
    }
}

/// Neither `sudo` nor UAC applies; run everything in-process.
#[cfg(not(any(unix, windows)))]
const fn prepare_elevation(
    _ctx: &Context,
    _log: &Arc<Logger>,
    _names: &[&str],
    _selectors: &[&str],
    _task_count: usize,
) -> ElevationPlan {
    ElevationPlan::Ready
}

/// Rewrite this run's arguments so the elevated child runs only `selectors`.
///
/// Existing `--only` / `--skip` filters are dropped because the child's scope is
/// decided here, and `--no-parallel` is forced so its output stays readable in
/// the separate console `Start-Process` opens. Every other flag — `--profile`,
/// `--root`, `--overlay`, verbosity — is preserved verbatim so the child sees
/// the same configuration as the parent.
///
/// Kept pure so the rewriting rules can be asserted on any platform.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "used by the Windows elevation broker")
)]
fn build_elevated_child_args(args: &[String], selectors: &[&str]) -> Vec<String> {
    /// Filters whose values the child must not inherit.
    const DROPPED: [&str; 2] = ["--only", "--skip"];

    let mut out: Vec<String> = Vec::with_capacity(args.len().saturating_add(4));
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if DROPPED.contains(&arg.as_str()) {
            skip_next = true;
            continue;
        }
        if DROPPED
            .iter()
            .any(|flag| arg.starts_with(&format!("{flag}=")))
        {
            continue;
        }
        if arg == "--no-parallel" || arg == "--elevated-child" {
            continue;
        }
        out.push(arg.clone());
    }

    out.push("--only".to_string());
    out.push(selectors.join(","));
    out.push("--no-parallel".to_string());
    out.push("--elevated-child".to_string());
    out
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

/// Tasks that cannot run because a prerequisite in `roots` will not run.
///
/// Returns each blocked task mapped to the name of the *root* that blocks it,
/// so the skip reason names something the user can act on rather than an
/// intermediate hop.
///
/// This is required because dropping a task from the slice is not the same as
/// failing it: [`ResolvedTaskGraph`](crate::engine::graph::ResolvedTaskGraph)
/// deliberately ignores dependencies that are absent from the slice, so a
/// dependent would otherwise run against a prerequisite that never happened.
/// The scheduler's own failure path already cascades; this restores the same
/// guarantee for tasks removed before the graph is built.
///
/// Iterates to a fixed point because the slice is not in topological order.
fn blocked_dependents<'a>(
    tasks: &[&'a dyn Task],
    roots: &HashMap<TaskId, &'a str>,
) -> HashMap<TaskId, &'a str> {
    let mut blocked: HashMap<TaskId, &'a str> = HashMap::new();
    loop {
        let mut discovered = false;
        for task in tasks {
            let id = task.task_id();
            if roots.contains_key(&id) || blocked.contains_key(&id) {
                continue;
            }
            let cause = task
                .dependencies()
                .iter()
                .find_map(|dep| roots.get(dep).or_else(|| blocked.get(dep)).copied());
            if let Some(cause) = cause {
                blocked.insert(id, cause);
                discovered = true;
            }
        }
        if !discovered {
            return blocked;
        }
    }
}

fn run_task_graph(tasks: &mut Vec<&dyn Task>, ctx: &Context, log: &Arc<Logger>) -> Result<()> {
    if ctx.is_cancelled() || tasks.is_empty() {
        return Ok(());
    }

    let elevating: Vec<&dyn Task> = if crate::infra::elevation::is_elevated_child() {
        // The child was spawned precisely to run these tasks; it must not
        // recurse into another elevation request.
        Vec::new()
    } else {
        tasks
            .iter()
            .filter(|task| crate::engine::requires_elevation(**task, ctx))
            .copied()
            .collect()
    };

    if !elevating.is_empty() {
        let names: Vec<&str> = elevating.iter().map(|task| task.name()).collect();
        let selectors: Vec<&str> = elevating.iter().map(|task| task.selector()).collect();
        let plan = prepare_elevation(ctx, log, &names, &selectors, tasks.len());

        // Delegation is not degradation: the tasks really ran, just in the
        // elevated child, so their dependents must still run here. Only an
        // unavailable plan leaves prerequisites unmet.
        let (reason, cascade) = match plan {
            ElevationPlan::Ready => (None, false),
            ElevationPlan::Delegated => (Some("ran in elevated session"), false),
            ElevationPlan::Unavailable { reason } => (Some(reason), true),
        };

        if let Some(reason) = reason {
            let roots: HashMap<TaskId, &str> = elevating
                .iter()
                .map(|task| (task.task_id(), task.name()))
                .collect();
            let blocked = if cascade {
                blocked_dependents(tasks, &roots)
            } else {
                HashMap::new()
            };

            tasks.retain(|task| {
                let id = task.task_id();
                let message = if roots.contains_key(&id) {
                    Some(reason.to_string())
                } else {
                    blocked.get(&id).map(|cause| format!("requires {cause}"))
                };
                let Some(message) = message else {
                    return true;
                };

                let span = tracing::info_span!("task", name = task.name());
                let _enter = span.enter();
                log.debug(message.as_str());
                log.record_task_with_metadata(
                    task.name(),
                    crate::infra::logging::TaskStatus::Skipped,
                    Some(message.as_str()),
                    ActionCounts::default(),
                    task.visibility(),
                );
                log.mark_task_completed(task.name());
                log.emit_task_result_and_redraw(task.name());
                false
            });
        }
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
