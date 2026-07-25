//! Structural invariants of the resolved task dependency graph.
//!
//! The hand-written cases in `engine::graph` cover the shapes someone thought
//! to write down. These tests instead assert the properties that must hold for
//! *every* graph, checked across a deterministic family of generated shapes, so
//! a scheduling regression cannot hide in a shape nobody enumerated.
//!
//! Generation is seeded and reproducible: a failure names the exact seed and
//! shape that broke.

use std::collections::HashSet;

use anyhow::Result;

use crate::engine::graph::{GraphError, ResolvedTaskGraph};
use crate::engine::{Context, Task, TaskId, TaskResult};

/// A task whose identity and dependencies are supplied at construction time.
///
/// [`TaskId::Dynamic`] makes it possible to build arbitrary graph shapes at
/// runtime, which type-derived ids cannot express.
struct GeneratedTask {
    name: String,
    id: TaskId,
    dependencies: Vec<TaskId>,
}

impl Task for GeneratedTask {
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
        Ok(TaskResult::Ok)
    }
}

/// Deterministic linear congruential generator (the numeric constants are the
/// well-known `Numerical Recipes` parameters).
struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from(self.0 >> 33).unwrap_or(u32::MAX)
    }

    /// Return `true` with probability `numerator / 100`.
    fn chance(&mut self, numerator: u32) -> bool {
        self.next_u32() % 100 < numerator
    }
}

/// Build `size` tasks whose edges always point from a lower index to a higher
/// one, which makes the graph acyclic by construction.
fn generate_dag(seed: u64, size: usize, edge_chance: u32) -> Vec<GeneratedTask> {
    let mut rng = Rng::new(seed);
    let mut tasks: Vec<GeneratedTask> = Vec::with_capacity(size);
    for idx in 0..size {
        let dependencies = (0..idx)
            .filter(|_| rng.chance(edge_chance))
            .map(|dep| TaskId::Dynamic(u64::try_from(dep).expect("index fits in u64")))
            .collect();
        tasks.push(GeneratedTask {
            name: format!("task-{idx}"),
            id: TaskId::Dynamic(u64::try_from(idx).expect("index fits in u64")),
            dependencies,
        });
    }
    tasks
}

fn as_dyn(tasks: &[GeneratedTask]) -> Vec<&dyn Task> {
    tasks.iter().map(|task| -> &dyn Task { task }).collect()
}

/// Seeds and shapes exercised by every invariant below.
const SHAPES: &[(u64, usize, u32)] = &[
    (1, 1, 50),
    (2, 2, 100),
    (3, 8, 10),
    (4, 8, 50),
    (5, 16, 25),
    (6, 32, 5),
    (7, 32, 40),
    (8, 64, 3),
];

#[test]
fn generated_dags_always_resolve() {
    for &(seed, size, edge_chance) in SHAPES {
        let tasks = generate_dag(seed, size, edge_chance);
        assert!(
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).is_ok(),
            "acyclic graph failed to resolve (seed {seed}, size {size})"
        );
    }
}

#[test]
fn execution_order_is_a_permutation_of_every_task() {
    for &(seed, size, edge_chance) in SHAPES {
        let tasks = generate_dag(seed, size, edge_chance);
        let graph =
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).expect("acyclic graph should resolve");

        let order: Vec<usize> = graph.execution_order().collect();
        assert_eq!(
            order.len(),
            size,
            "execution order dropped tasks (seed {seed}, size {size})"
        );
        let unique: HashSet<usize> = order.iter().copied().collect();
        assert_eq!(
            unique.len(),
            size,
            "execution order repeated a task (seed {seed}, size {size})"
        );
    }
}

#[test]
fn every_dependency_is_scheduled_before_its_dependent() {
    for &(seed, size, edge_chance) in SHAPES {
        let tasks = generate_dag(seed, size, edge_chance);
        let graph =
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).expect("acyclic graph should resolve");

        let mut position = vec![usize::MAX; size];
        for (slot, task_idx) in graph.execution_order().enumerate() {
            position[task_idx] = slot;
        }

        for task_idx in 0..size {
            for &dep_idx in graph.dependencies(task_idx) {
                assert!(
                    position[dep_idx] < position[task_idx],
                    "task {task_idx} scheduled before dependency {dep_idx} \
                     (seed {seed}, size {size})"
                );
            }
        }
    }
}

#[test]
fn dependents_are_the_exact_reverse_of_dependencies() {
    for &(seed, size, edge_chance) in SHAPES {
        let tasks = generate_dag(seed, size, edge_chance);
        let graph =
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).expect("acyclic graph should resolve");

        let mut forward: HashSet<(usize, usize)> = HashSet::new();
        let mut reverse: HashSet<(usize, usize)> = HashSet::new();
        for task_idx in 0..size {
            for &dep_idx in graph.dependencies(task_idx) {
                forward.insert((dep_idx, task_idx));
            }
            for &dependent_idx in graph.dependents(task_idx) {
                reverse.insert((task_idx, dependent_idx));
            }
        }
        assert_eq!(
            forward, reverse,
            "dependency and dependent edge sets disagree (seed {seed}, size {size})"
        );
    }
}

#[test]
fn adding_a_back_edge_always_produces_a_cycle() {
    for &(seed, size, edge_chance) in SHAPES {
        if size < 2 {
            continue;
        }
        let mut tasks = generate_dag(seed, size, edge_chance);

        // Force a path from the first task to the last, then close it with a
        // back edge from the first task to the last.
        for (idx, task) in tasks.iter_mut().enumerate().skip(1) {
            let dep = TaskId::Dynamic(u64::try_from(idx - 1).expect("index fits in u64"));
            if !task.dependencies.contains(&dep) {
                task.dependencies.push(dep);
            }
        }
        tasks[0].dependencies.push(TaskId::Dynamic(
            u64::try_from(size - 1).expect("index fits in u64"),
        ));

        assert_eq!(
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).err(),
            Some(GraphError::Cycle),
            "back edge did not produce a cycle (seed {seed}, size {size})"
        );
    }
}

#[test]
fn a_self_dependency_is_a_cycle() {
    let tasks = vec![GeneratedTask {
        name: "self".to_string(),
        id: TaskId::Dynamic(0),
        dependencies: vec![TaskId::Dynamic(0)],
    }];
    assert_eq!(
        ResolvedTaskGraph::resolve(&as_dyn(&tasks)).err(),
        Some(GraphError::Cycle)
    );
}

#[test]
fn duplicate_ids_are_rejected_regardless_of_shape() {
    for &(seed, size, edge_chance) in SHAPES {
        if size < 2 {
            continue;
        }
        let mut tasks = generate_dag(seed, size, edge_chance);
        tasks[size - 1].id = TaskId::Dynamic(0);

        assert_eq!(
            ResolvedTaskGraph::resolve(&as_dyn(&tasks)).err(),
            Some(GraphError::DuplicateId),
            "duplicate id not rejected (seed {seed}, size {size})"
        );
    }
}

#[test]
fn dependencies_outside_the_filtered_slice_are_ignored() {
    // Command filters can remove a dependency from the active task list; the
    // remaining tasks must still resolve against each other.
    let tasks = vec![
        GeneratedTask {
            name: "present".to_string(),
            id: TaskId::Dynamic(0),
            dependencies: vec![TaskId::Dynamic(99)],
        },
        GeneratedTask {
            name: "dependent".to_string(),
            id: TaskId::Dynamic(1),
            dependencies: vec![TaskId::Dynamic(0), TaskId::Dynamic(99)],
        },
    ];

    let graph = ResolvedTaskGraph::resolve(&as_dyn(&tasks)).expect("missing deps are not an error");
    assert!(graph.dependencies(0).is_empty());
    assert_eq!(graph.dependencies(1), &[0]);
    assert_eq!(graph.execution_order().collect::<Vec<_>>(), vec![0, 1]);
}
