//! Task dependency graph utilities.
#![allow(
    clippy::indexing_slicing,
    reason = "resolved graph indices and topological in-degrees are validated by graph construction"
)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::engine::{Task, TaskAssessment, TaskId};

/// Reason a task dependency graph failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// Two or more tasks share the same [`TaskId`], so dependencies cannot be
    /// resolved unambiguously.
    #[error("duplicate task identifier {} used by '{first}' and '{second}'", .id.record_key())]
    DuplicateId {
        /// Colliding scheduler identity.
        id: TaskId,
        /// First task using the identity.
        first: String,
        /// Second task using the identity.
        second: String,
    },
    /// The dependency graph contains at least one cycle.
    #[error("dependency cycle: {}", .path.join(" -> "))]
    Cycle {
        /// Closed task-name path, including the first node again at the end.
        path: Vec<String>,
    },
}

/// Dependency graph resolved against one filtered task slice.
///
/// Missing dependencies are intentionally ignored: command filters can remove a
/// dependency from the active task list, and the remaining tasks should be
/// scheduled relative to the tasks that are still present.
#[derive(Debug)]
pub(crate) struct ResolvedTaskGraph {
    ids: Vec<TaskId>,
    id_to_idx: HashMap<TaskId, usize>,
    blocking_dependencies: Vec<Vec<usize>>,
    dependencies: Vec<Vec<usize>>,
    dependents: Vec<Vec<usize>>,
    execution_order: Vec<usize>,
}

/// Dependency edges followed by a graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependencyEdges {
    /// Follow only failure-blocking dependencies.
    Blocking,
    /// Follow blocking and ordering-only dependencies.
    All,
}

impl ResolvedTaskGraph {
    /// Build and validate the graph for `tasks`.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::DuplicateId`] if two tasks share a [`TaskId`], or
    /// [`GraphError::Cycle`] if the graph contains at least one dependency cycle.
    pub(crate) fn resolve(tasks: &[&dyn Task]) -> Result<Self, GraphError> {
        Self::resolve_assessed(tasks, &HashMap::new())
    }

    /// Resolve execution edges from immutable phase assessments. Static graph
    /// discovery and selection continue to include every declared dependency.
    pub(crate) fn resolve_assessed(
        tasks: &[&dyn Task],
        assessments: &HashMap<TaskId, TaskAssessment>,
    ) -> Result<Self, GraphError> {
        let mut id_to_idx: HashMap<TaskId, usize> = HashMap::new();
        let mut ids = Vec::with_capacity(tasks.len());
        for (idx, task) in tasks.iter().enumerate() {
            let id = task.task_id();
            if let Some(&first_idx) = id_to_idx.get(&id) {
                return Err(GraphError::DuplicateId {
                    id,
                    first: tasks[first_idx].name().to_string(),
                    second: task.name().to_string(),
                });
            }
            id_to_idx.insert(id.clone(), idx);
            ids.push(id);
        }

        let blocking_dependencies: Vec<Vec<usize>> = tasks
            .iter()
            .map(|task| resolve_dependencies(task.dependencies(), &id_to_idx))
            .collect();
        let dependencies: Vec<Vec<usize>> = tasks
            .iter()
            .zip(&blocking_dependencies)
            .map(|(task, blocking)| {
                let mut resolved = blocking.clone();
                extend_resolved_dependencies(
                    &mut resolved,
                    &task
                        .ordering_dependencies()
                        .iter()
                        .filter(|id| {
                            !assessments
                                .get(&task.task_id())
                                .is_some_and(|assessment| assessment.ordering_satisfied(id))
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                    &id_to_idx,
                );
                resolved
            })
            .collect();

        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
        for (task_idx, deps) in dependencies.iter().enumerate() {
            for &dep_idx in deps {
                dependents[dep_idx].push(task_idx);
            }
        }

        let execution_order =
            topological_order(&dependencies, &dependents).ok_or_else(|| GraphError::Cycle {
                path: find_cycle_path(&dependencies, tasks),
            })?;

        Ok(Self {
            ids,
            id_to_idx,
            blocking_dependencies,
            dependencies,
            dependents,
            execution_order,
        })
    }

    /// Whether the active graph contains this scheduler identity.
    #[must_use]
    pub(crate) fn contains(&self, task_id: &TaskId) -> bool {
        self.id_to_idx.contains_key(task_id)
    }

    /// Add every reachable dependency of the selected tasks.
    pub(crate) fn extend_dependency_closure(
        &self,
        selected: &mut HashSet<TaskId>,
        edges: DependencyEdges,
    ) {
        let mut pending = selected
            .iter()
            .filter_map(|task_id| self.id_to_idx.get(task_id).copied())
            .collect::<Vec<_>>();

        while let Some(task_idx) = pending.pop() {
            let dependencies = match edges {
                DependencyEdges::Blocking => self.blocking_dependencies(task_idx),
                DependencyEdges::All => self.dependencies(task_idx),
            };
            for &dependency_idx in dependencies {
                let dependency_id = &self.ids[dependency_idx];
                if selected.insert(dependency_id.clone()) {
                    pending.push(dependency_idx);
                }
            }
        }
    }

    /// Tasks transitively blocked by a set of unmet prerequisite roots.
    pub(crate) fn blocked_dependents<'task>(
        &self,
        roots: &HashMap<TaskId, &'task str>,
    ) -> HashMap<TaskId, &'task str> {
        let mut blocked = HashMap::new();
        for task_idx in self.execution_order() {
            let task_id = &self.ids[task_idx];
            if roots.contains_key(task_id) {
                continue;
            }
            // Every prerequisite has been visited. Prefer the first declared
            // blocking dependency with a cause, independent of catalog order.
            let cause = self
                .blocking_dependencies(task_idx)
                .iter()
                .map(|dependency_idx| &self.ids[*dependency_idx])
                .find_map(|dependency_id| {
                    roots
                        .get(dependency_id)
                        .or_else(|| blocked.get(dependency_id))
                        .copied()
                });
            if let Some(cause) = cause {
                blocked.insert(task_id.clone(), cause);
            }
        }
        blocked
    }

    /// Task indices this task depends on.
    #[must_use]
    pub(crate) fn dependencies(&self, task_idx: usize) -> &[usize] {
        &self.dependencies[task_idx]
    }

    fn blocking_dependencies(&self, task_idx: usize) -> &[usize] {
        &self.blocking_dependencies[task_idx]
    }

    /// Task indices that depend on this task.
    #[must_use]
    pub(crate) fn dependents(&self, task_idx: usize) -> &[usize] {
        &self.dependents[task_idx]
    }

    /// Whether failure of `dependency_idx` blocks `task_idx`.
    #[must_use]
    pub(crate) fn blocks_on_failure(&self, task_idx: usize, dependency_idx: usize) -> bool {
        self.blocking_dependencies(task_idx)
            .contains(&dependency_idx)
    }

    /// Return task indices in dependency-safe execution order.
    pub(crate) fn execution_order(&self) -> impl Iterator<Item = usize> + '_ {
        self.execution_order.iter().copied()
    }
}

fn resolve_dependencies(dependencies: &[TaskId], id_to_idx: &HashMap<TaskId, usize>) -> Vec<usize> {
    let mut resolved = Vec::new();
    extend_resolved_dependencies(&mut resolved, dependencies, id_to_idx);
    resolved
}

fn extend_resolved_dependencies(
    resolved: &mut Vec<usize>,
    dependencies: &[TaskId],
    id_to_idx: &HashMap<TaskId, usize>,
) {
    for dependency in dependencies {
        if let Some(&dependency_idx) = id_to_idx.get(dependency)
            && !resolved.contains(&dependency_idx)
        {
            resolved.push(dependency_idx);
        }
    }
}

fn topological_order(dependencies: &[Vec<usize>], dependents: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut in_degree: Vec<usize> = dependencies.iter().map(Vec::len).collect();
    let mut queue: VecDeque<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(idx, &degree)| (degree == 0).then_some(idx))
        .collect();
    let mut order = Vec::with_capacity(dependencies.len());

    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for &dependent_idx in &dependents[idx] {
            let count = &mut in_degree[dependent_idx];
            *count -= 1;
            if *count == 0 {
                queue.push_back(dependent_idx);
            }
        }
    }

    (order.len() == dependencies.len()).then_some(order)
}

#[allow(
    clippy::expect_used,
    clippy::unreachable,
    reason = "the DFS stack and state values are established by this traversal"
)]
fn find_cycle_path(dependencies: &[Vec<usize>], tasks: &[&dyn Task]) -> Vec<String> {
    fn visit(
        node: usize,
        dependencies: &[Vec<usize>],
        states: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        states[node] = 1;
        stack.push(node);
        for &dependency in &dependencies[node] {
            match states[dependency] {
                0 => {
                    if let Some(path) = visit(dependency, dependencies, states, stack) {
                        return Some(path);
                    }
                }
                1 => {
                    let start = stack
                        .iter()
                        .position(|&item| item == dependency)
                        .expect("active dependency must be on the DFS stack");
                    let mut path = stack[start..].to_vec();
                    path.push(dependency);
                    return Some(path);
                }
                2 => {}
                _ => unreachable!("DFS state must be unvisited, active, or complete"),
            }
        }
        stack.pop();
        states[node] = 2;
        None
    }

    let mut states = vec![0; dependencies.len()];
    let mut stack = Vec::new();
    for node in 0..dependencies.len() {
        if states[node] == 0
            && let Some(path) = visit(node, dependencies, &mut states, &mut stack)
        {
            return path
                .into_iter()
                .map(|idx| tasks[idx].name().to_string())
                .collect();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    use crate::engine::{Context, TaskId, TaskMeta, TaskResult};

    use anyhow::Result;

    // -----------------------------------------------------------------------
    // Mock tasks — each is a distinct type so TaskId-based deps work.
    // -----------------------------------------------------------------------

    crate::test_helpers::test_task! {
        TaskA {
            name: "a",
        }
        TaskB {
            name: "b",
        }
        TaskC {
            name: "c",
        }
        DepA {
            name: "dep-a",
        }
        DepB {
            name: "dep-b",
            deps: [DepA],
        }
        DepC {
            name: "dep-c",
            deps: [DepB],
        }
        DiaA {
            name: "dia-a",
        }
        DiaB {
            name: "dia-b",
            deps: [DiaA],
        }
        DiaC {
            name: "dia-c",
            deps: [DiaA],
        }
        DiaD {
            name: "dia-d",
            deps: [DiaB, DiaC],
        }
        CycA {
            name: "cyc-a",
            deps: [CycB],
        }
        CycB {
            name: "cyc-b",
            deps: [CycA],
        }
        MissingDepTask {
            name: "missing-dep",
            deps: [DepC],
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
        assert!(matches!(
            validate(&tasks),
            Err(GraphError::Cycle { ref path })
                if path == &["cyc-a", "cyc-b", "cyc-a"]
                    || path == &["cyc-b", "cyc-a", "cyc-b"]
        ));
    }

    #[test]
    fn missing_dep_not_a_cycle() {
        let tasks: Vec<&dyn Task> = vec![&MissingDepTask, &TaskA];
        assert_eq!(validate(&tasks), Ok(()));
    }

    crate::test_helpers::test_task! {
        DuplicateIdA { name: "duplicate-a", }
    }

    struct DuplicateIdB;
    impl Task for DuplicateIdB {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("duplicate-b")
        }
        fn task_id(&self) -> TaskId {
            // Deliberately returns DuplicateIdA's TypeId to simulate a collision.
            TaskId::Type(TypeId::of::<DuplicateIdA>())
        }
        fn dependencies(&self) -> &[TaskId] {
            const DEPS: &[TaskId] = &[TaskId::Type(TypeId::of::<DuplicateIdA>())];
            DEPS
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn duplicate_task_ids_are_treated_as_invalid() {
        let tasks: Vec<&dyn Task> = vec![&DuplicateIdA, &DuplicateIdB];
        assert!(matches!(
            validate(&tasks),
            Err(GraphError::DuplicateId {
                ref first,
                ref second,
                ..
            }) if first == "duplicate-a" && second == "duplicate-b"
        ));
    }
    #[test]
    fn assessed_graph_waives_only_selected_ordering_edges() {
        let task = crate::engine::TaskWithExtraDeps::new(
            Box::new(TaskC),
            &[TaskA.task_id()],
            &[TaskA.task_id(), TaskB.task_id()],
        );
        let tasks: &[&dyn Task] = &[&TaskA, &TaskB, &task];
        let static_graph = ResolvedTaskGraph::resolve(tasks).unwrap();
        assert_eq!(static_graph.dependencies(2), &[0, 1]);
        let assessments = HashMap::from([(
            task.task_id(),
            TaskAssessment::applicable()
                .with_satisfied_ordering(&[TaskA.task_id(), TaskB.task_id()]),
        )]);
        let graph = ResolvedTaskGraph::resolve_assessed(tasks, &assessments).unwrap();
        assert_eq!(graph.dependencies(2), &[0]);
        assert!(graph.blocks_on_failure(2, 0));
        // Selection still follows the full static graph.
        let mut selected = HashSet::from([task.task_id()]);
        static_graph.extend_dependency_closure(&mut selected, DependencyEdges::All);
        assert!(selected.contains(&TaskB.task_id()));
    }
}
