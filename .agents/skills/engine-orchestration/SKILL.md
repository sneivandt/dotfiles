---
name: engine-orchestration
description: >
  Use for task graph and execution changes in cli/src/engine/, command task
  membership, dependencies, dynamic task discovery, Operation plans,
  ProcessMode, or task/resource parallelism. Not for a standalone Resource.
---

# Engine Orchestration

## Choose the boundary

| Need | Owner |
|---|---|
| identity, eligibility, elevation, dependencies | `Task` |
| independent items with separate state | `Resource` |
| one idempotent multi-step workflow | `Operation` |
| pure parsing or transformation | plain module/function |

Use `resource-implementation` for a concrete resource that does not alter
scheduling.

## Task graph rules

- `Task::meta()` is the source for `TaskId`, selector, display name, visibility,
  and update-only status. Prefer `task_metadata!`.
- Dependencies are the only ordering policy; catalog order is irrelevant.
- `dependencies()` block on predecessor failure.
  `ordering_dependencies()` wait without propagating failure.
- Dynamic instances use `TaskId::dynamic::<Self>(stable_key)` and are created at
  the owning discovery boundary, after any prerequisite config reload.
- `should_run()` and `needs_elevation()` must be cheap and side-effect free.
  State produced by a prerequisite is checked in `run_configured()`.
- Removing unmet work before graph execution must also skip its transitive
  dependents; an absent node does not propagate failure.

## Convergence rules

- Use `process_resources*()` for item-shaped work.
- Use `Operation` plus `process_operation()` for workflow-shaped work.
- An operation computes one immutable plan in `current_state()` and passes that
  exact plan to `preview()` or `apply()`; never recompute it.
- Keep config read guards out of long-running or parallel work.
- Task parallelism uses scoped threads; resource parallelism uses Rayon.
  `ctx.parallel` gates both.

## Process modes

| Mode | Behavior |
|---|---|
| `Strict` | fix missing/incorrect; stop on error |
| `Lenient` | fix missing/incorrect; continue independent work |
| `InstallMissing` | install missing only |
| `FixExisting` | fix existing only |

Use the matching `ProcessOpts` constructor and an existing canonical verb.

For focused graph, filtering, discovery, and operation-plan coverage, use the
`task_execution` and affected command suites listed under
[Integration test suites](../../../docs/TESTING.md#integration-test-suites).
