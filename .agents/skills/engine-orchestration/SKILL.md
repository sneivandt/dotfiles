---
name: engine-orchestration
description: >
  Use for task graph and execution changes in cli/src/engine/, command task
  membership, dependencies, startup task discovery, restart boundaries, Operation plans,
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
- Keep identity, CLI selector, and display label distinct. A label change must
  not silently rename a selector or change a dependency identity.
- Dependencies are the only ordering policy; catalog order is irrelevant.
- Declare same-domain edges in the task and cross-domain edges in the
  [application catalog](../../../cli/src/app/catalog.rs), not through sibling
  domain imports.
- `dependencies()` block on predecessor failure.
  `ordering_dependencies()` wait without propagating failure.
- Dynamic instances use `TaskId::dynamic::<Self>(stable_key)` and are created
  once from the immutable startup configuration.
- A repository update that changes task/config inputs must use the guarded
  process restart boundary. Do not mutate config handles or discover tasks late.
- In the restarted child, remove repository synchronization only after target
  selection, then run the remaining selected tasks from the fresh configuration
  snapshot.
- Applicability and elevation are assessed once per execution phase. Keep
  `should_run()` and `needs_elevation()` read-only and use phase-stable state;
  avoid unnecessary expensive probes. Check state produced by a prerequisite
  in `run()` / `run_configured()` after it finishes.
- Removing unmet work before graph execution must also skip its transitive
  dependents; an absent node does not propagate failure.

## Convergence rules

- Use `process_resources*()` for item-shaped work.
- Use `Operation` plus `process_operation()` for workflow-shaped work.
- An operation computes one immutable plan in `current_state()` and passes that
  exact plan to `preview()` or `apply()`; never recompute it.
- `ConfigHandle::read()` returns a cheap `Arc` clone of an immutable snapshot,
  not a reloadable lock. Give domain tasks only their typed slice; do not
  introduce mutable shared configuration or unnecessary deep config copies.
- Task parallelism uses scoped threads; resource parallelism uses Rayon.
  `ctx.parallel()` gates both. Resource `.sequential()` protects shared-file or
  lock-bound writes; cross-task contention needs explicit graph policy.

## Process modes

| Mode | Behavior |
|---|---|
| `Strict` | fix missing/incorrect; stop on error |
| `Lenient` | fix missing/incorrect; continue independent work |
| `InstallMissing` | install missing only; count errors and continue |
| `FixExisting` | fix incorrect existing state only; stop on error |

Use the matching `ProcessOpts` constructor and an existing canonical verb.
Lenient continuation never erases failed-item accounting.

Read [operation lifecycle](../../../cli/src/engine/operation.rs) and
[processing modes](../../../cli/src/engine/mode.rs) when changing convergence.
Cover selection (`--only`, `--skip`, update-only membership), dependency failure,
ordering-only continuation, internal visibility, and sequential/parallel parity
as applicable. Use
`task_execution` and affected command suites listed under
[Integration test suites](../../../docs/TESTING.md#integration-test-suites).
Update [Task reference](../../../docs/TASKS.md) when public task metadata or
command membership changes.
