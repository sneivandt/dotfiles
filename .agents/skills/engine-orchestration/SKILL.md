---
name: engine-orchestration
description: >
  Task and operation orchestration in the dotfiles engine. Use when changing
  task dependencies, command membership, scheduling, dynamic task discovery,
  ProcessMode, Operation workflows, parallel execution, or the Rayon-based
  resource pipeline.
---

# Engine Orchestration

## Use this skill when

- changing task dependencies, command membership, or scheduler behavior
- changing late task discovery around a dependency boundary
- deciding between `Task` + resources and `Task` + `Operation`
- changing resource processing strategy (`process_resources*`, `ProcessMode`)

## Do not use this skill when

- implementing a concrete resource type without scheduler changes (use
  `resource-implementation`)
- defining test structure/layout details (use `testing-patterns`)

## Decision guide / invariants

- Keep identity, command membership, eligibility, elevation prediction, and
  dependencies in `Task`; keep convergence logic in resources or `Operation`.
- Ordering comes only from explicit dependency edges. Catalog order is not
  scheduling policy.
- `update_only()` controls whether a task belongs only to `dotfiles update`; it
  does not create an ordering barrier.
- Task-level parallelism uses scoped OS threads; resource-level parallelism uses
  Rayon.
- `ctx.parallel` gates both levels.
- Resource tasks should use orchestration helpers from `engine/orchestrate.rs`
  (re-exported by `engine/mod.rs`) instead of custom dry-run/apply loops.

## Implementation procedure / core patterns

1. **Task graph:** resolve with `ResolvedTaskGraph::resolve()` and fail on
   duplicate IDs/cycles.
2. **Scheduler wiring:** keep dependency channels strict; failed dependency
   blocks dependents.
3. **Command membership:** filter update-only tasks before applying `--only`
   and `--skip`; filtering must not expand hidden prerequisites.
4. **Dynamic discovery:** when refreshed configuration can change the task set,
   run the discovery boundary's dependency closure, rebuild dynamic tasks, then
   schedule them with remaining static tasks. If the boundary is absent after
   filtering, discover before running one graph.
5. **Task execution path:** route all tasks through `engine::execute()` so
   `should_run()` owns execution eligibility. Override `run_configured()` only
   when an otherwise-applicable task can have no configured work; do not repeat
   eligibility checks there.
6. **Resource flow:** use one of:
   - `process_resources(...)`
   - `process_resources_with_provider(...)`
   - `process_resources_remove(...)`
7. **Operation flow:** use `Operation` + `process_operation()` when convergence is
   workflow-shaped, not item-shaped. State discovery returns
   `OperationState<Plan>`, and the engine passes the same immutable plan to
   preview or apply; do not cache or recompute it inside the operation.

### Process mode reference

| Mode | `fix_incorrect()` | `fix_missing()` | `bail_on_error()` | Typical use |
|---|---|---|---|---|
| `Strict` | yes | yes | yes | symlinks, hooks, git config |
| `Lenient` | yes | yes | no | packages, registry, developer mode |
| `InstallMissing` | no | yes | no | VS Code extensions, systemd units |
| `FixExisting` | yes | no | yes | chmod |

Use `ProcessOpts::{strict,lenient,install_missing,fix_existing}` with canonical
verbs (`install`, `configure`, `update`, `enable`, `link`, `unlink`, `remove`).

## Validation

Add targeted scheduler and orchestration tests under `cli/src/engine/` and the
affected task modules.

## Common mistakes / anti-patterns

- Running subprocesses outside the executor abstraction
- Holding a config read guard during long-running or parallel work
- Mutating in `should_run()`
- Duplicating resource-processing dry-run logic in task bodies
- Adding static tasks without catalog registration
- Relying on catalog order instead of declaring a dependency
- Using `update_only()` as an ordering mechanism
- Rebuilding dynamic tasks before refreshed configuration is available
- Adding conditional symlink behavior without matching manifest coverage
- Hardcoded OS checks where capability methods exist
