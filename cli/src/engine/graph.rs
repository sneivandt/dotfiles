//! Task dependency graph utilities.

use std::collections::{HashMap, VecDeque};

use crate::tasks::{Task, TaskId};

/// Reason a task dependency graph failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    /// Two or more tasks share the same [`TaskId`], so dependencies cannot be
    /// resolved unambiguously.
    DuplicateId,
    /// The dependency graph contains at least one cycle.
    Cycle,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId => f.write_str("duplicate task identifiers"),
            Self::Cycle => f.write_str("dependency cycle"),
        }
    }
}

impl std::error::Error for GraphError {}

/// Dependency graph resolved against one filtered task slice.
///
/// Missing dependencies are intentionally ignored: command filters can remove a
/// dependency from the active task list, and the remaining tasks should be
/// scheduled relative to the tasks that are still present.
#[derive(Debug)]
pub(crate) struct ResolvedTaskGraph {
    dependencies: Vec<Vec<usize>>,
    dependents: Vec<Vec<usize>>,
}

impl ResolvedTaskGraph {
    /// Build and validate the graph for `tasks`.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::DuplicateId`] if two tasks share a [`TaskId`], or
    /// [`GraphError::Cycle`] if the graph contains at least one dependency cycle.
    pub(crate) fn resolve(tasks: &[&dyn Task]) -> Result<Self, GraphError> {
        let id_to_idx: HashMap<TaskId, usize> = tasks
            .iter()
            .enumerate()
            .map(|(idx, task)| (task.task_id(), idx))
            .collect();

        if id_to_idx.len() != tasks.len() {
            return Err(GraphError::DuplicateId);
        }

        let dependencies: Vec<Vec<usize>> = tasks
            .iter()
            .map(|task| {
                task.dependencies()
                    .iter()
                    .filter_map(|dep| id_to_idx.get(dep).copied())
                    .collect()
            })
            .collect();

        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
        for (task_idx, deps) in dependencies.iter().enumerate() {
            for &dep_idx in deps {
                if let Some(reverse) = dependents.get_mut(dep_idx) {
                    reverse.push(task_idx);
                }
            }
        }

        let graph = Self {
            dependencies,
            dependents,
        };
        graph.validate_acyclic()?;
        Ok(graph)
    }

    /// Task indices this task depends on.
    #[must_use]
    pub(crate) fn dependencies(&self, task_idx: usize) -> &[usize] {
        self.dependencies.get(task_idx).map_or(&[], Vec::as_slice)
    }

    /// Task indices that depend on this task.
    #[must_use]
    pub(crate) fn dependents(&self, task_idx: usize) -> &[usize] {
        self.dependents.get(task_idx).map_or(&[], Vec::as_slice)
    }

    /// Return task indices in dependency-safe execution order.
    #[must_use]
    pub(crate) fn execution_order(&self) -> Vec<usize> {
        let mut in_degree: Vec<usize> = self.dependencies.iter().map(Vec::len).collect();
        let mut queue: VecDeque<usize> = in_degree
            .iter()
            .enumerate()
            .filter_map(|(idx, &degree)| (degree == 0).then_some(idx))
            .collect();
        let mut order = Vec::with_capacity(self.dependencies.len());

        while let Some(idx) = queue.pop_front() {
            order.push(idx);
            for &dependent_idx in self.dependents(idx) {
                if let Some(count) = in_degree.get_mut(dependent_idx) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        queue.push_back(dependent_idx);
                    }
                }
            }
        }

        debug_assert_eq!(order.len(), self.dependencies.len());
        order
    }

    fn validate_acyclic(&self) -> Result<(), GraphError> {
        let mut in_degree: Vec<usize> = self.dependencies.iter().map(Vec::len).collect();
        let mut queue: Vec<usize> = in_degree
            .iter()
            .enumerate()
            .filter_map(|(idx, &degree)| (degree == 0).then_some(idx))
            .collect();
        let mut processed = 0usize;

        while let Some(idx) = queue.pop() {
            processed = processed.saturating_add(1);
            for &dependent_idx in self.dependents(idx) {
                if let Some(count) = in_degree.get_mut(dependent_idx) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        queue.push(dependent_idx);
                    }
                }
            }
        }

        if processed == self.dependencies.len() {
            Ok(())
        } else {
            Err(GraphError::Cycle)
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use std::any::TypeId;

    use crate::tasks::{Context, TaskId, TaskPhase, TaskResult};

    use anyhow::Result;

    // -----------------------------------------------------------------------
    // Mock tasks — each is a distinct type so TaskId-based deps work.
    // -----------------------------------------------------------------------

    macro_rules! mock_task {
        ($name:ident, $display:expr, $deps:expr) => {
            struct $name;
            impl Task for $name {
                fn name(&self) -> &str {
                    $display
                }
                fn phase(&self) -> TaskPhase {
                    TaskPhase::Provision
                }
                fn dependencies(&self) -> &[TaskId] {
                    const DEPS: &[TaskId] = $deps;
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
    mock_task!(DepB, "dep-b", &[TaskId::Type(TypeId::of::<DepA>())]);
    mock_task!(DepC, "dep-c", &[TaskId::Type(TypeId::of::<DepB>())]);

    // Diamond: DiaA → DiaB + DiaC → DiaD
    mock_task!(DiaA, "dia-a", &[]);
    mock_task!(DiaB, "dia-b", &[TaskId::Type(TypeId::of::<DiaA>())]);
    mock_task!(DiaC, "dia-c", &[TaskId::Type(TypeId::of::<DiaA>())]);
    mock_task!(
        DiaD,
        "dia-d",
        &[
            TaskId::Type(TypeId::of::<DiaB>()),
            TaskId::Type(TypeId::of::<DiaC>())
        ]
    );

    // Cyclic: CycA → CycB → CycA
    mock_task!(CycA, "cyc-a", &[TaskId::Type(TypeId::of::<CycB>())]);
    mock_task!(CycB, "cyc-b", &[TaskId::Type(TypeId::of::<CycA>())]);

    // Missing dep
    struct MissingDepTask;
    impl Task for MissingDepTask {
        fn name(&self) -> &'static str {
            "missing-dep"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
        }
        fn dependencies(&self) -> &[TaskId] {
            // Points to a TaskId that won't be present in the task list
            const DEPS: &[TaskId] = &[TaskId::Type(TypeId::of::<DepC>())];
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
    // validate
    // -----------------------------------------------------------------------

    fn validate(tasks: &[&dyn Task]) -> Result<(), GraphError> {
        ResolvedTaskGraph::resolve(tasks).map(|_| ())
    }

    #[test]
    fn no_cycle_independent_tasks() {
        let tasks: Vec<&dyn Task> = vec![&TaskA, &TaskB, &TaskC];
        assert_eq!(validate(&tasks), Ok(()));
    }

    #[test]
    fn no_cycle_linear_chain() {
        let tasks: Vec<&dyn Task> = vec![&DepA, &DepB, &DepC];
        assert_eq!(validate(&tasks), Ok(()));
    }

    #[test]
    fn no_cycle_diamond() {
        let tasks: Vec<&dyn Task> = vec![&DiaA, &DiaB, &DiaC, &DiaD];
        assert_eq!(validate(&tasks), Ok(()));
    }

    #[test]
    fn cycle_detected() {
        let tasks: Vec<&dyn Task> = vec![&CycA, &CycB];
        assert_eq!(validate(&tasks), Err(GraphError::Cycle));
    }

    #[test]
    fn missing_dep_not_a_cycle() {
        let tasks: Vec<&dyn Task> = vec![&MissingDepTask, &TaskA];
        assert_eq!(validate(&tasks), Ok(()));
    }

    struct DuplicateIdA;
    impl Task for DuplicateIdA {
        fn name(&self) -> &'static str {
            "duplicate-a"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct DuplicateIdB;
    impl Task for DuplicateIdB {
        fn name(&self) -> &'static str {
            "duplicate-b"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
        }
        fn task_id(&self) -> TaskId {
            // Deliberately returns DuplicateIdA's TypeId to simulate a collision.
            TaskId::Type(TypeId::of::<DuplicateIdA>())
        }
        fn dependencies(&self) -> &[TaskId] {
            const DEPS: &[TaskId] = &[TaskId::Type(TypeId::of::<DuplicateIdA>())];
            DEPS
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn duplicate_task_ids_are_treated_as_invalid() {
        let tasks: Vec<&dyn Task> = vec![&DuplicateIdA, &DuplicateIdB];
        assert_eq!(validate(&tasks), Err(GraphError::DuplicateId));
    }

    // -----------------------------------------------------------------------
    // install order: verify real tasks form a valid DAG
    // -----------------------------------------------------------------------

    #[test]
    fn install_tasks_have_resolvable_dependencies() {
        use std::collections::HashSet;
        let tasks = crate::tasks::all_install_tasks();
        let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TaskId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate task TaskIds found");
        let present: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        for task in &tasks {
            for dep in task.dependencies() {
                assert!(
                    present.contains(dep),
                    "task '{}' depends on a TaskId not in the task list",
                    task.name()
                );
            }
        }
    }

    #[test]
    fn install_tasks_have_no_cycles() {
        let tasks = crate::tasks::all_install_tasks();
        let task_refs: Vec<&dyn Task> = tasks.iter().map(Box::as_ref).collect();
        assert_eq!(
            validate(&task_refs),
            Ok(()),
            "install task graph should be a valid DAG"
        );
    }
}
