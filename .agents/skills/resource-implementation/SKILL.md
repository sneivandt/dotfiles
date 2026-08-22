---
name: resource-implementation
description: >
  Use when adding or changing a concrete Resource, IntrinsicState, or
  ResourceStateProvider under cli/src/domains/*/resources/. Not for scheduler
  policy or a whole-workflow Operation.
---

# Resource Implementation

## Select the model

| Requirement | Model |
|---|---|
| each item discovers its own state | `Resource` + `IntrinsicState` |
| one expensive query supplies many items | `ResourceStateProvider` |
| one multi-step workflow converges as a unit | `Operation` via `engine-orchestration` |

## Required behavior

- State discovery is read-only and returns
  `ResourceResult<ResourceState>`.
- `apply()` and `remove()` return `ResourceResult<ResourceChange>`.
- Use `Missing`, `Correct`, `Incorrect`, `Invalid`, and `Unknown` precisely;
  never turn an unknown or unsafe state into success.
- Use `ResourceChange::skipped(reason)` for a deliberate benign no-op and
  `ResourceChange::unusable(reason)` for unmet work. Do not construct the
  skipped variant directly.
- Route subprocesses through the executor and environment reads through an
  injected `ctx.env()` handle.
- Keep platform-specific mutation inside the resource; the task chooses
  applicability and process policy.

## Wire the vertical slice

1. Implement the resource in its domain `resources/` module.
2. Update typed config and validation when the resource is config-backed.
3. Call `run_resource_task()` or `run_batch_resource_task()` from a `Task`.
4. Select the narrowest `ProcessOpts`.
5. Export modules and register static install/uninstall tasks.
6. Add state, mutation, dry-run, and failure tests.

Canonical contracts live in `cli/src/engine/resource/`,
`cli/src/engine/orchestrate.rs`, and `cli/src/engine/plan.rs`.

Use `error-handling-patterns` when result classification changes and
`cross-platform-verification` after implementation.
