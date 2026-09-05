---
name: resource-implementation
description: >
  Use when adding or changing a concrete Resource, RemovableResource,
  IntrinsicState, ResourceStateProvider, or its resource-task adapter under
  cli/src/domains/. Not for scheduler policy or a whole-workflow Operation.
---

# Resource Implementation

## Select the model

| Requirement | Model |
|---|---|
| each item discovers its own state | `Resource` + `IntrinsicState` |
| one expensive query supplies many items | `ResourceStateProvider` |
| a resource supports uninstall | additionally implement `RemovableResource` |
| one multi-step workflow converges as a unit | `Operation` via `engine-orchestration` |

## Required behavior

- State discovery is read-only and returns
  `ResourceResult<ResourceState>`.
- `Resource::apply()` returns `ResourceResult<ResourceChange>`. Implement
  `remove()` on `RemovableResource`, not on `Resource`; do not add a stub removal
  that fails at runtime.
- Use `Missing`, `Correct`, `Incorrect`, `Invalid`, and `Unknown` precisely;
  never turn an unknown or unsafe state into success.
- The engine owns check/plan/dry-run/apply sequencing. Keep writes, directory
  creation, and other mutations out of constructors and state discovery.
- Preserve `pre_apply_warning()` for destructive replacement. Removal normally
  touches only `Correct` resources; `remove_when_missing()` is an explicit opt-in
  for resources such as symlinks that materialize content during uninstall.
- Route subprocesses through the executor and environment reads through an
  injected `ctx.env()` handle.
- Keep platform-specific mutation inside the resource; the task chooses
  applicability and process policy.
- Use the existing task adapter (`run_resource_task()` or
  `run_batch_resource_task()`) where it fits; otherwise use `process_resources*()`.
  Pick `ProcessOpts` deliberately and use `.sequential()` when items share a
  file or exclusive lock.

Read the [resource contracts](../../../cli/src/engine/resource/contract.rs),
[processing entry points](../../../cli/src/engine/orchestrate.rs), and
[state-to-plan mapping](../../../cli/src/engine/plan.rs), plus the closest
resource and its tests, before implementing.

Cover missing, correct, incorrect, invalid, and unknown state as applicable;
assert that dry-run invokes no mutation and a second apply converges. For
removal, cover both managed and user-replaced targets. Use
[Testing](../../../docs/TESTING.md#choosing-coverage) to select coverage.
