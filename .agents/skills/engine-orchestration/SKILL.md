---
name: engine-orchestration
description: >
  Task scheduling and resource parallelism in the dotfiles engine. Use when
  working with task dependencies, parallel execution, the scheduler, or
  the Rayon-based resource processing pipeline.
---

# Engine Orchestration

## Use this skill when

- changing task phase ordering, dependencies, or scheduler behavior
- deciding between `Task` + resources and `Task` + `Operation`
- changing resource processing strategy (`process_resources*`, `ProcessMode`)

## Do not use this skill when

- implementing a concrete resource type without scheduler changes (use
  `resource-implementation`)
- defining test structure/layout details (use `testing-patterns`)

## Decision guide / invariants

- Keep metadata/policies/dependencies in `Task`; keep convergence logic in
  resources or `Operation`.
- Phase barrier is strict (`Bootstrap -> Sync -> Provision -> Validation -> Update`).
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
3. **Task execution path:** route all tasks through `engine::execute()` so policy
   and `should_run()` are evaluated once. Override `run_configured()` only when
   an otherwise-applicable task can have no configured work; do not repeat policy
   or guard checks there.
4. **Resource flow:** use one of:
   - `process_resources(...)`
   - `process_resources_with_provider(...)`
   - `process_resources_remove(...)`
5. **Operation flow:** use `Operation` + `process_operation()` when convergence is
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

- Use `cross-platform-verification` for canonical local Rust/cross-platform
  checks.
- Add/adjust targeted scheduler and orchestration tests under `cli/src/engine/`
  and affected task modules.

## Common mistakes / anti-patterns

- Running subprocesses outside the executor abstraction
- Holding a config read guard during long-running or parallel work
- Mutating in `should_run()`
- Duplicating resource-processing dry-run logic in task bodies
- Adding static tasks without catalog registration
- Adding conditional symlink behavior without matching manifest coverage
- Hardcoded OS checks where capability methods exist
- Duplicating the canonical validation sequence in multiple skills

## Related skills

- `resource-implementation`
- `error-handling-patterns`
- `logging-patterns`
- `cross-platform-verification`
