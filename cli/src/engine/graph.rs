//! Task dependency graph utilities.

use std::any::TypeId;
use std::collections::HashMap;

use crate::tasks::Task;

/// Detect cycles in the task dependency graph using Kahn's algorithm.
///
/// Returns `true` if the graph contains at least one cycle or if task
/// identifiers are not unique.
pub fn has_cycle(tasks: &[&dyn Task]) -> bool {
    let type_to_idx: HashMap<TypeId, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.task_id(), i))
        .collect();

    if type_to_idx.len() != tasks.len() {
        return true;
    }

    let mut in_degree: Vec<usize> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| type_to_idx.contains_key(d))
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
        .filter_map(|(i, &d)| (d == 0).then_some(i))
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::{Context, TaskPhase, TaskResult};

    use anyhow::Result;

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
                fn phase(&self) -> TaskPhase {
                    TaskPhase::Apply
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
        fn name(&self) -> &'static str {
            "missing-dep"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
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

    struct DuplicateIdA;
    impl Task for DuplicateIdA {
        fn name(&self) -> &'static str {
            "duplicate-a"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
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
            TaskPhase::Apply
        }
        fn task_id(&self) -> TypeId {
            TypeId::of::<DuplicateIdA>()
        }
        fn dependencies(&self) -> &[TypeId] {
            const DEPS: &[TypeId] = &[TypeId::of::<DuplicateIdA>()];
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
        assert!(has_cycle(&tasks));
    }

    // -----------------------------------------------------------------------
    // install order: verify real tasks form a valid DAG
    // -----------------------------------------------------------------------

    #[test]
    fn install_tasks_have_resolvable_dependencies() {
        use std::collections::HashSet;
        let tasks = crate::tasks::all_install_tasks();
        let ids: Vec<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TypeId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate task TypeIds found");
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
