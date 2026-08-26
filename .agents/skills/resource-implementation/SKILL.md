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
- Result classification and mutation ordering belong to
  `error-handling-patterns`.
- Route subprocesses through the executor and environment reads through an
  injected `ctx.env()` handle.
- Keep platform-specific mutation inside the resource; the task chooses
  applicability and process policy.
- Call `run_resource_task()` or `run_batch_resource_task()` with the narrowest
  `ProcessOpts`.

Canonical contracts live in `cli/src/engine/resource/`,
`cli/src/engine/orchestrate.rs`, and `cli/src/engine/plan.rs`.

Use [Testing](../../../docs/TESTING.md#choosing-coverage) to select resource and
command coverage.
