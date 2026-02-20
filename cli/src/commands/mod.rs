pub mod install;
pub mod test;
pub mod uninstall;

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use anyhow::Result;

use crate::cli::GlobalOpts;
use crate::config::Config;
use crate::config::profiles;
use crate::logging::{BufferedLog, Logger};
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Shared state produced by the common command setup sequence.
///
/// Encapsulates platform detection, profile resolution, and configuration
/// loading so that each command does not have to repeat the boilerplate.
#[derive(Debug)]
pub struct CommandSetup {
    pub platform: Platform,
    pub config: Config,
}

impl CommandSetup {
    /// Detect the platform, resolve the profile, and load all configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the root directory cannot be determined, the profile
    /// cannot be resolved, or any configuration file fails to parse.
    pub fn init(global: &GlobalOpts, log: &Logger) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;

        log.stage("Resolving profile");
        let profile = profiles::resolve_from_args(global.profile.as_deref(), &root, &platform)?;
        log.info(&format!("profile: {}", profile.name));

        log.stage("Loading configuration");
        let config = Config::load(&root, &profile, &platform)?;

        log.debug(&format!("{} packages", config.packages.len()));
        log.debug(&format!("{} symlinks", config.symlinks.len()));
        log.debug(&format!("{} registry entries", config.registry.len()));
        log.debug(&format!("{} systemd units", config.units.len()));
        log.debug(&format!("{} chmod entries", config.chmod.len()));
        log.debug(&format!(
            "{} vscode extensions",
            config.vscode_extensions.len()
        ));
        log.debug(&format!("{} copilot skills", config.copilot_skills.len()));
        log.debug(&format!(
            "{} manifest exclusions",
            config.manifest.excluded_files.len()
        ));
        log.info(&format!(
            "loaded {} packages, {} symlinks",
            config.packages.len(),
            config.symlinks.len()
        ));

        // Validate configuration and display warnings
        let warnings = config.validate(&platform);
        if !warnings.is_empty() {
            log.warn(&format!(
                "found {} configuration warning(s):",
                warnings.len()
            ));
            for warning in &warnings {
                log.warn(&format!(
                    "  {} [{}]: {}",
                    warning.source, warning.item, warning.message
                ));
            }
        }

        Ok(Self { platform, config })
    }
}

/// Shared state for dependency-driven parallel task scheduling.
///
/// Tasks call [`wait_for_deps`](TaskGraph::wait_for_deps) before starting and
/// [`mark_complete`](TaskGraph::mark_complete) when finished.  The [`Condvar`]
/// wakes all waiting tasks whenever a new completion is recorded.
#[derive(Debug)]
struct TaskGraph {
    /// Set of completed task [`TypeId`]s.
    completed: Mutex<HashSet<TypeId>>,
    /// Notified whenever a task completes.
    condvar: Condvar,
}

impl TaskGraph {
    fn new() -> Self {
        Self {
            completed: Mutex::new(HashSet::new()),
            condvar: Condvar::new(),
        }
    }

    /// Block until every [`TypeId`] in `deps` has been marked complete.
    fn wait_for_deps(&self, deps: &[TypeId]) {
        if deps.is_empty() {
            return;
        }
        let mut completed = self
            .completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !deps.iter().all(|d| completed.contains(d)) {
            completed = self
                .condvar
                .wait(completed)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(completed);
    }

    /// Record a task as complete and wake all waiting threads.
    fn mark_complete(&self, id: TypeId) {
        let mut completed = self
            .completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        completed.insert(id);
        drop(completed);
        self.condvar.notify_all();
    }
}

/// Detect cycles in the task dependency graph using Kahn's algorithm.
///
/// Returns `true` if the graph contains at least one cycle.
fn has_cycle(tasks: &[&dyn Task]) -> bool {
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    let type_to_idx: HashMap<TypeId, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.task_id(), i))
        .collect();

    let mut in_degree: Vec<usize> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| present.contains(d))
                .count()
        })
        .collect();

    let mut reverse_deps: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    for (i, t) in tasks.iter().enumerate() {
        for dep in t.dependencies() {
            if let Some(&dep_idx) = type_to_idx.get(dep)
                && let Some(rd) = reverse_deps.get_mut(dep_idx)
            {
                rd.push(i);
            }
        }
    }

    let mut queue: Vec<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let mut processed = 0usize;

    while let Some(idx) = queue.pop() {
        processed += 1;
        if let Some(dependents) = reverse_deps.get(idx) {
            for &dep in dependents {
                if let Some(count) = in_degree.get_mut(dep) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push(dep);
                    }
                }
            }
        }
    }

    processed != tasks.len()
}

/// Execute every task respecting dependency order.
///
/// When parallel execution is enabled and more than one task is present,
/// tasks run as soon as their dependencies complete.  Each task's console
/// output is buffered and flushed atomically on completion.  A status line
/// shows which tasks are currently running.
///
/// When parallel execution is disabled (or only one task is present),
/// tasks execute sequentially in list order.
///
/// # Errors
///
/// Returns an error if one or more tasks recorded a failure.
pub fn run_tasks_to_completion<'a>(
    tasks: impl IntoIterator<Item = &'a dyn Task>,
    ctx: &Context,
    log: &Logger,
) -> Result<()> {
    let tasks: Vec<&dyn Task> = tasks.into_iter().collect();

    if ctx.parallel && tasks.len() > 1 {
        if has_cycle(&tasks) {
            log.warn("dependency cycle detected; falling back to sequential execution");
            for task in &tasks {
                tasks::execute(*task, ctx);
            }
        } else {
            run_tasks_parallel(&tasks, ctx, log);
        }
    } else {
        for task in &tasks {
            tasks::execute(*task, ctx);
        }
    }

    log.print_summary();

    let count = log.failure_count();
    if count > 0 {
        anyhow::bail!("{count} task(s) failed");
    }
    Ok(())
}

/// Run tasks in parallel using a dependency graph.
///
/// Each task is spawned into an OS thread (via `std::thread::scope`) and waits
/// for its dependencies to complete before executing.  OS threads are used
/// deliberately — blocking on a `Condvar` inside a Rayon worker would exhaust
/// Rayon's fixed-size thread pool and deadlock when the pool is smaller than
/// the number of tasks with unsatisfied dependencies (common on 2-vCPU CI
/// runners).  Output is buffered per-task and flushed to the console
/// immediately on completion.
fn run_tasks_parallel(tasks: &[&dyn Task], ctx: &Context, log: &Logger) {
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    let resolved_deps: Vec<Vec<TypeId>> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| present.contains(d))
                .copied()
                .collect()
        })
        .collect();

    let graph = TaskGraph::new();

    std::thread::scope(|s| {
        for (task, deps) in tasks.iter().zip(resolved_deps.iter()) {
            let task = *task;
            let graph = &graph;
            s.spawn(move || {
                graph.wait_for_deps(deps);

                log.notify_task_start(task.name());

                let buf = BufferedLog::new(log);
                let task_ctx = Context {
                    config: Arc::clone(&ctx.config),
                    platform: ctx.platform,
                    log: &buf,
                    dry_run: ctx.dry_run,
                    home: ctx.home.clone(),
                    executor: ctx.executor,
                    parallel: ctx.parallel,
                };
                tasks::execute(task, &task_ctx);

                buf.flush_and_complete(task.name());
                graph.mark_complete(task.task_id());
            });
        }
    });
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::{Context, TaskResult};

    // -----------------------------------------------------------------------
    // Mock tasks — each is a distinct type so TypeId-based deps work.
    // -----------------------------------------------------------------------

    macro_rules! mock_task {
        ($name:ident, $display:expr, $deps:expr) => {
            struct $name;
            impl Task for $name {
                fn name(&self) -> &str {
                    $display
                }
                fn dependencies(&self) -> &[TypeId] {
                    const DEPS: &[TypeId] = $deps;
                    DEPS
                }
                fn should_run(&self, _ctx: &Context) -> bool {
                    true
                }
                fn run(&self, _ctx: &Context) -> Result<TaskResult> {
                    Ok(TaskResult::Ok)
                }
            }
        };
    }

    // Simple tasks for basic tests
    mock_task!(TaskA, "a", &[]);
    mock_task!(TaskB, "b", &[]);
    mock_task!(TaskC, "c", &[]);

    // Chain: DepA → DepB → DepC
    mock_task!(DepA, "dep-a", &[]);
    mock_task!(DepB, "dep-b", &[TypeId::of::<DepA>()]);
    mock_task!(DepC, "dep-c", &[TypeId::of::<DepB>()]);

    // Diamond: DiaA → DiaB + DiaC → DiaD
    mock_task!(DiaA, "dia-a", &[]);
    mock_task!(DiaB, "dia-b", &[TypeId::of::<DiaA>()]);
    mock_task!(DiaC, "dia-c", &[TypeId::of::<DiaA>()]);
    mock_task!(DiaD, "dia-d", &[TypeId::of::<DiaB>(), TypeId::of::<DiaC>()]);

    // Cyclic: CycA → CycB → CycA
    mock_task!(CycA, "cyc-a", &[TypeId::of::<CycB>()]);
    mock_task!(CycB, "cyc-b", &[TypeId::of::<CycA>()]);

    // Missing dep
    struct MissingDepTask;
    impl Task for MissingDepTask {
        fn name(&self) -> &str {
            "missing-dep"
        }
        fn dependencies(&self) -> &[TypeId] {
            // Points to a TypeId that won't be present in the task list
            const DEPS: &[TypeId] = &[TypeId::of::<DepC>()];
            DEPS
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    // -----------------------------------------------------------------------
    // TaskGraph
    // -----------------------------------------------------------------------

    #[test]
    fn graph_no_deps_does_not_block() {
        let graph = TaskGraph::new();
        graph.wait_for_deps(&[]);
    }

    #[test]
    fn graph_satisfied_deps_do_not_block() {
        let graph = TaskGraph::new();
        let id = TypeId::of::<TaskA>();
        graph.mark_complete(id);
        graph.wait_for_deps(&[id]);
    }

    #[test]
    fn graph_notifies_waiters() {
        let graph = std::sync::Arc::new(TaskGraph::new());
        let id = TypeId::of::<TaskA>();
        let g = std::sync::Arc::clone(&graph);
        let handle = std::thread::spawn(move || {
            g.wait_for_deps(&[id]);
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        graph.mark_complete(id);
        handle.join().expect("waiter thread should complete");
    }

    #[test]
    fn graph_multiple_deps_all_required() {
        let graph = std::sync::Arc::new(TaskGraph::new());
        let id_a = TypeId::of::<TaskA>();
        let id_b = TypeId::of::<TaskB>();
        let g = std::sync::Arc::clone(&graph);
        let handle = std::thread::spawn(move || {
            g.wait_for_deps(&[id_a, id_b]);
        });
        graph.mark_complete(id_a);
        // Only one dep satisfied — thread should still be waiting.
        std::thread::sleep(std::time::Duration::from_millis(50));
        graph.mark_complete(id_b);
        handle.join().expect("waiter thread should complete");
    }

    // -----------------------------------------------------------------------
    // has_cycle
    // -----------------------------------------------------------------------

    #[test]
    fn no_cycle_independent_tasks() {
        let tasks: Vec<&dyn Task> = vec![&TaskA, &TaskB, &TaskC];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn no_cycle_linear_chain() {
        let tasks: Vec<&dyn Task> = vec![&DepA, &DepB, &DepC];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn no_cycle_diamond() {
        let tasks: Vec<&dyn Task> = vec![&DiaA, &DiaB, &DiaC, &DiaD];
        assert!(!has_cycle(&tasks));
    }

    #[test]
    fn cycle_detected() {
        let tasks: Vec<&dyn Task> = vec![&CycA, &CycB];
        assert!(has_cycle(&tasks));
    }

    #[test]
    fn missing_dep_not_a_cycle() {
        let tasks: Vec<&dyn Task> = vec![&MissingDepTask, &TaskA];
        assert!(!has_cycle(&tasks));
    }

    // -----------------------------------------------------------------------
    // install order: verify real tasks form a valid DAG
    // -----------------------------------------------------------------------

    #[test]
    fn install_tasks_have_resolvable_dependencies() {
        let tasks = crate::tasks::all_install_tasks();
        let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
        for task in &tasks {
            for dep in task.dependencies() {
                assert!(
                    present.contains(dep),
                    "task '{}' depends on a TypeId not in the task list",
                    task.name()
                );
            }
        }
    }

    #[test]
    fn install_tasks_have_no_cycles() {
        let tasks = crate::tasks::all_install_tasks();
        let task_refs: Vec<&dyn Task> = tasks.iter().map(Box::as_ref).collect();
        assert!(
            !has_cycle(&task_refs),
            "install task graph should not contain cycles"
        );
    }
}
