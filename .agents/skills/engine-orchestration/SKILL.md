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
- Keep task metadata distinct: `TaskId` is scheduler identity, `selector()` is
  the stable CLI interface, `name()` is the display label, and `visibility()`
  controls discovery, normal rows, and totals.
- All four are derived from a single required `Task::meta() -> TaskMeta<'_>`.
  Implement `meta()` (usually via `task_metadata!`) and leave `name()`,
  `selector()`, `visibility()`, and `update_only()` as the defaults, so a
  decorator that forwards `meta()` cannot silently drop part of a task's
  identity.
- `--only` exact-matches normalized selectors, with exact full-label matching
  retained for compatibility. Do not add action-prefix, first-word, or substring
  matching.
- `dotfiles tasks` merges visible command membership by selector in discovery
  order. Conflicting labels for one selector are errors; internal tasks are not
  discoverable or selectable.
- Ordering comes only from explicit dependency edges. Catalog order is not
  scheduling policy. Completed rows remain in natural completion order; do not
  sort or regroup them.
- `update_only()` controls whether a task belongs only to `dotfiles update`; it
  does not create an ordering barrier.
- The coordinator calls `Task::assess()` once per task and execution phase, then
  shares the immutable `TaskAssessment` with elevation preparation and dispatch.
  Keep `should_run()` and `needs_elevation()` free of side effects.
- Assessment happens before the phase graph runs. Eligibility and elevation
  probes must therefore depend only on state stable for that phase. If a
  prerequisite can create a tool or state needed by a successor, defer that
  check to `run_configured()` after dependencies complete and return `Ok(None)`
  when it remains unavailable.
- `dependencies()` edges are failure-blocking prerequisites.
  `ordering_dependencies()` edges wait for completion but do not propagate
  predecessor failure.
- Dynamic task instances use `TaskId::dynamic::<Self>(stable_key)` rather than
  hashes. Display names need not be unique; scheduler records are keyed by
  `TaskId`.
- Task-level parallelism uses scoped OS threads; resource-level parallelism uses
  Rayon.
- `ctx.parallel` gates both levels.
- Cancellation records every task that has not started as skipped with reason
  `cancelled` and propagates that signal through dependents. When dependency
  failure and cancellation are both observed, dependency failure takes
  precedence.
- The resolved graph deliberately ignores dependencies that are absent from the
  filtered slice. Removing a task from the slice is therefore **not** the same as
  failing it: the scheduler's dependent cascade never fires. Any pre-graph
  removal that represents unmet work — such as a task skipped because elevation
  was unavailable — must also remove that task's transitive dependents and
  record them as skipped with the originating reason.
- Resource tasks should use orchestration helpers from `engine/orchestrate.rs`
  (re-exported by `engine/mod.rs`) instead of custom dry-run/apply loops.

## Implementation procedure / core patterns

1. **Task graph:** resolve with `ResolvedTaskGraph::resolve()` and fail on
   duplicate IDs/cycles.
2. **Scheduler wiring:** preserve whether each edge is failure-blocking or
   ordering-only; only blocking edges propagate failure.
3. **Command membership:** filter update-only tasks before applying `--only`
   and `--skip`; filtering must not expand hidden prerequisites. Preserve
   selector uniqueness and visibility rules in discovery.
4. **Dynamic discovery:** when refreshed configuration can change the task set,
   run the discovery boundary's dependency closure, rebuild dynamic tasks, then
   schedule them with remaining static tasks. If the boundary is absent after
   filtering, discover before running one graph.
5. **Task execution path:** route application graphs through cached
   `TaskAssessment` and `execute_assessed()`. Override `run_configured()` when an
   otherwise-applicable task can have no configured work or must check state
   produced by a prerequisite; do not repeat stable eligibility checks there.
6. **Resource flow:** use one of:
   - `process_resources(...)`
   - `process_resources_with_cache(...)`
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
- Overriding `name()`/`selector()`/`visibility()`/`update_only()` instead of
  `meta()`, or writing a decorator that forwards individual accessors rather
  than `meta()`
- Reading `std::env` from task or resource code instead of `ctx.env()`
- Rebuilding dynamic tasks before refreshed configuration is available
- Adding conditional symlink behavior without matching manifest coverage
- Hardcoded OS checks where capability methods exist
