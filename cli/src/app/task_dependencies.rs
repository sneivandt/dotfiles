//! Shared task dependency traversal for selection and restart boundaries.

use std::collections::{HashMap, HashSet};

use crate::engine::{Task, TaskId};

/// Dependency edges followed while extending a selected task set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependencyEdges {
    /// Follow only failure-blocking dependencies.
    Blocking,
    /// Follow both blocking and ordering-only dependencies.
    All,
}

/// Add every reachable dependency of the selected tasks.
pub(crate) fn extend_dependency_closure(
    tasks: &[&dyn Task],
    selected: &mut HashSet<TaskId>,
    edges: DependencyEdges,
) {
    let by_id = tasks
        .iter()
        .map(|task| (task.task_id(), *task))
        .collect::<HashMap<_, _>>();
    let mut pending = selected.iter().cloned().collect::<Vec<_>>();

    while let Some(task_id) = pending.pop() {
        let Some(task) = by_id.get(&task_id) else {
            continue;
        };
        let mut include = |dependency: &TaskId| {
            if by_id.contains_key(dependency) && selected.insert(dependency.clone()) {
                pending.push(dependency.clone());
            }
        };
        for dependency in task.dependencies() {
            include(dependency);
        }
        if edges == DependencyEdges::All {
            for dependency in task.ordering_dependencies() {
                include(dependency);
            }
        }
    }
}
