---
name: engine-orchestration
description: >
  Use for task graph, scheduling, and concrete Resource/Operation work in
  cli/src/engine/ and cli/src/domains/: Task/Resource/RemovableResource/Operation
  contracts, Requirement probes, resource-task adapters, dependencies,
  AccessClaim exclusion, RuntimePolicy, restart boundaries, ProcessMode, or
  parallelism.
---

# Engine Orchestration and Resources

Preserve layer rules enforced by [domain_boundaries](../../../cli/tests/domain_boundaries.rs)
and see [Architecture](../../../docs/content/development/architecture.md#repository-layout).
Key engine files: [resource contracts](../../../cli/src/engine/resource/contract.rs),
[orchestrate](../../../cli/src/engine/orchestrate.rs),
[plan](../../../cli/src/engine/plan.rs),
[requirement](../../../cli/src/engine/requirement.rs),
[operation](../../../cli/src/engine/operation.rs),
[mode](../../../cli/src/engine/mode.rs), and
[task macros](../../../cli/src/engine/task/macros.rs).

## Runtime policy

- Resolve engine-command flags and environment/terminal decisions once through
  `app/commands/runtime.rs::RuntimePolicy` and pass `ExecutionPolicy` into
  `Context::new_with_policy`. Do not re-read `CI` or recompute prompt/child
  guards in profile, re-exec, or elevation callers.
- In fixtures, inject `RuntimePolicy::new` with explicit stdin/stdout
  capabilities; keep elevated-window exit-pause tests separate from task prompt
  policy. Preserve `CI`-presence semantics and the distinct nonempty elevated
  marker rule.

## Task graph and execution

- Keep `Task::task_id()` (scheduler edges), `Task::meta()` selector (CLI),
  display label, and `Task::log_key()` (persistent log identity) distinct;
  decorators must forward `log_key()`. Dynamic tasks use
  `TaskId::dynamic::<Self>(stable_key)` from the immutable startup snapshot.
- `dependencies()` block on predecessor failure; `ordering_dependencies()` wait
  without propagating failure. Conditional ordering lives in `TaskWithExtraDeps`
  (`Task::ordering_requirements()` + application-selected provider-stability
  requirement) and populates `TaskAssessment` once per phase; `resolve_assessed`
  uses it while static discovery/selection keeps all declared edges. Never waive
  blocking edges or mutate in readiness probes; unknown state retains the wait.
- `should_run()` and `needs_elevation()` are evaluated once per phase from
  phase-stable state—check prerequisite-produced state in `run()` after the
  prerequisite finishes. Removing unmet work before execution skips its
  transitive dependents without propagating failure.
- Guarded process restart owns config/task reloads after a repository update:
  in the restarted child, strip repository sync only after target selection.
  Never mutate `ConfigHandle` or discover tasks mid-run.
- `Operation` computes one immutable plan in `current_state()` and passes that
  exact plan to `preview()` or `apply()`; never recompute it.
- `Requirement` probes (`process_requirement()`) are read-only and stage-aware
  (`Ordering`, `Preview`, `Apply`); consume their typed evidence rather than
  re-querying, and never reuse `Ordering` evidence during execution.
- Task scheduling uses a bounded worker pool; resource parallelism uses a
  separate Rayon pool (`ctx.parallel()` gates both). Declare a claimed task's
  full nested `AccessClaim` set up front (nested resources must be subsets), and
  keep claim waits on the caller thread—never inside Rayon workers. `.sequential()`
  is batch-local only and does not synchronize across tasks.

## Resources

- Use `config_task!` for single-slice task structs, `run_resource_task()` /
  `run_batch_resource_task()` (or `process_resources*()` /
  `process_resources_with_cache()`) for item-shaped work, and `Operation` +
  `process_operation()` for single-unit workflows.
- Implement `remove()` only via `RemovableResource`, never as a stub on
  `Resource`. Removal normally touches only `Correct` resources;
  `remove_when_missing()` is an explicit opt-in (e.g., symlinks materializing
  content during uninstall).
- Use `pre_apply_warning()` only for actionable warnings (symlink replacement
  intentionally emits only the normal link action).
- Declare `Resource::access_claims()` whenever items share a document or
  exclusive state so discovery through mutation is protected across tasks. Never
  populate a batch cache from mutable claimed state before acquiring access.
- Adapters return `NotApplicable` for empty item lists; the executor owns task
  stage records.

## Verification

Add shared scheduler parity cases to
[`conformance.rs`](../../../cli/src/engine/tests/scheduler/conformance.rs)
(runs sequential and parallel modes and compares outcomes/records); keep
concurrency synchronization in `parallel.rs` and start/result ordering in
`output.rs`. See [Writing tests](../../../docs/content/development/writing-tests.md)
for harness conventions and update
[Task reference](../../../docs/content/reference/task-reference.md) when public
task metadata or command membership changes.
