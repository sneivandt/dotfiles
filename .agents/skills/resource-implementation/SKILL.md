---
name: resource-implementation
description: >
  Patterns for implementing concrete Resource, IntrinsicState, and
  ResourceStateProvider types in cli/src/resources/. Use when adding a new
  resource or modifying existing resource behaviour.
---

# Resource Implementation

## Use this skill when

- adding/modifying a concrete resource in `cli/src/resources/`
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
| metadata/scheduling/dependencies/policies | `Task` |
| pure parsing/transformation | plain function/module |

## Invariants

- `Resource::apply/remove` return `ResourceResult<ResourceChange>` for typed,
  classifiable failures.
- State checking remains separate from mutation (`IntrinsicState` or provider).
- Tasks own metadata/policies/dependencies; resources own item-level state and
  convergence.
- Use executor abstraction for subprocesses.

Canonical references:
- `cli/src/resources/mod.rs`
- `cli/src/engine/orchestrate.rs`
- `cli/src/engine/mode.rs`
- `cli/src/tasks/macros.rs`

## Implementation procedure / core patterns

1. Implement resource struct in `cli/src/resources/<name>.rs`.
2. Implement `Resource` and choose:
   - `IntrinsicState` when each item can check itself
   - `ResourceStateProvider` when one cached query should feed many resources
3. Add/adjust config section in `cli/src/config/` and `conf/<name>.toml`.
4. Wire task (prefer `resource_task!`), select `ProcessOpts`, and register static
   task in `cli/src/tasks/catalog.rs`.
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
- `Skipped { reason }`

## Validation

- Use `cross-platform-verification` for canonical Rust/cross-platform checks.
- Add targeted tests for state mapping, apply/remove behavior, and task wiring.

## Common mistakes / anti-patterns

- subprocess calls outside executor abstraction
- config read guards held during long/parallel work
- mutation logic in `should_run()`
- duplicated resource-processing dry-run logic
- static task added without catalog registration
- conditional symlink handling without manifest coverage
- hardcoded OS checks where capability helpers exist
- duplicated canonical validation sequence

## Related skills

- `engine-orchestration`
- `error-handling-patterns`
- `toml-configuration`
- `config-validation`
- `testing-patterns`
