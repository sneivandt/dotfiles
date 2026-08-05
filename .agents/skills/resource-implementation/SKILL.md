---
name: resource-implementation
description: >
  Patterns for implementing concrete Resource, IntrinsicState, and
  ResourceStateProvider types in domain resource modules. Use when adding a new
  resource or modifying existing resource behaviour.
---

# Resource Implementation

## Use this skill when

- adding/modifying a concrete resource in `cli/src/domains/<domain>/resources/`
- deciding between `IntrinsicState` and `ResourceStateProvider`
- wiring config-backed resource tasks through orchestration helpers

## Do not use this skill when

- the work is mostly scheduling/dependency orchestration (use
  `engine-orchestration`)
- the convergence unit is whole-workflow, not per-item state (use `Operation`
  via `engine-orchestration`)

## Model selection

| Requirement | Choose |
|---|---|
| independent items with individual state | `Resource` (+ usually `IntrinsicState`) |
| expensive shared state query | `ResourceStateProvider` |
| idempotent multi-step workflow | `Operation` |
| identity/command membership/eligibility/dependencies | `Task` |
| pure parsing/transformation | plain function/module |

## Invariants

- `Resource::apply/remove` return `ResourceResult<ResourceChange>` for typed,
  classifiable failures.
- State discovery (`IntrinsicState::current_state` /
  `ResourceStateProvider::current_state`) also returns
  `ResourceResult<ResourceState>`, so a discovery failure carries the same
  `ResourceError::category()` the summary uses for mutation failures.
- State checking remains separate from mutation (`IntrinsicState` or provider).
- Tasks own identity, command membership, eligibility, elevation prediction,
  and dependencies; resources own item-level state and convergence.
- Use executor abstraction for subprocesses.
- Read environment variables through `ctx.env()` (an `infra::env::Env` handle),
  never `std::env` inline. Construct resources with the handle so tests can
  inject `MapEnv`.

Canonical references:
- `cli/src/engine/resource/`
- `cli/src/engine/orchestrate.rs`
- `cli/src/engine/mode.rs` (`ProcessMode` / `ProcessOpts`)
- `cli/src/engine/plan.rs` (`ApplyChange::from_state`, the state machine)
- `cli/src/engine/task/macros.rs` (`task_metadata!`, `run_resource_task()`)

## Implementation procedure / core patterns

1. Implement the resource in its owning domain's `resources/` module.
2. Implement `Resource` and choose:
   - `IntrinsicState` when each item can check itself
   - `ResourceStateProvider` when one cached query should feed many resources
3. Add/adjust the owning domain's config module and `conf/<name>.toml`.
4. Wire the task as a hand-written `impl Task` that calls
   `run_resource_task()` (or `run_batch_resource_task()` for cached state),
   select `ProcessOpts`, and register static tasks in `cli/src/app/catalog.rs`.
5. Export modules from the relevant `mod.rs` files.
6. Add or update focused tests (resource + config/task wiring as needed).

### `ResourceState` use

- `Missing`: not present
- `Correct`: already desired
- `Incorrect { current }`: present but wrong
- `Invalid { reason }`: known unsafe/invalid to apply
- `Unknown { reason }`: unable to determine

### `ResourceChange` use

- `Applied`
- `AlreadyCorrect`
- `ResourceChange::skipped(reason)` — a deliberate, benign no-op (unsupported
  platform, nothing configured, a target the resource refuses to touch by
  design). Counted as a skip.
- `ResourceChange::unusable(reason)` — the resource could not converge and the
  run should treat it as unmet work (missing privileges, an unavailable
  provider). Counted as a failure in the summary and exit status.

Both construct `ResourceChange::Skipped { reason, failed }`; never build that
variant literally — pick the constructor that matches the outcome, because the
`failed` flag is what decides whether the run reports success.

## Validation

- Add targeted tests for state mapping, apply/remove behavior, and task wiring.

## Common mistakes / anti-patterns

Resource-specific:

- mutating inside `IntrinsicState`/`ResourceStateProvider` state checks instead
  of `apply`/`remove`
- returning untyped errors from `apply`/`remove` or from `current_state`
  instead of `ResourceError`
- constructing `ResourceChange::Skipped { .. }` directly instead of using
  `ResourceChange::skipped()` / `ResourceChange::unusable()`, which silently
  mis-reports whether the run had unmet work
- reading `std::env` inside a resource instead of taking an
  `Arc<dyn infra::env::Env>` from `ctx.env()`
- hand-rolling dry-run/apply loops in the task body instead of using
  `process_resources*`

See `engine-orchestration` for the shared task/orchestration anti-patterns
(executor abstraction, config read guards, `should_run()` mutation, catalog
registration, manifest coverage, capability helpers).
