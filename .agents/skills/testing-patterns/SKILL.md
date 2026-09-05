---
name: testing-patterns
description: >
  Use when adding, changing, or restructuring Rust tests, fixtures, mocks,
  snapshots, or test utilities in cli/. Not for choosing CI jobs or the general
  validation command sequence.
---

# Testing Patterns

## Place tests

- Keep small cohesive tests in inline `#[cfg(test)] mod tests`.
- Move large sibling suites to `tests.rs`.
- For a domain-root task file, use `domains/<domain>/tests/<feature>.rs` with a
  test-only `#[path]`; never use `#[path]` for production wiring.
- Do not create a feature directory solely to hold tests.

## Reuse the harness

- Unit context/config helpers: `cli/src/app/test_helpers/`
- Integration helpers: `cli/tests/common/mod.rs`
- Executor results: `ExecResult::success` / `ExecResult::failure`
- Ordered command responses: `ScriptedExecutor`; its `.git()` expectations check
  exact Git arguments, cwd, and checked mode, while `.ok()` / `.err()` do not
  match the command. It reports executables as unavailable.
- Branching or unordered execution: `MockExecutor`
- Common task assertions: `assert_task_ok`, `assert_task_changed`,
  `task_batch`, `task_skipped`
- APM setup: `domains/ai/apm/test_fixture.rs`
- Environment behavior: inject `MapEnv`; never mutate process-global env

Prefer named case tables for repeated input/output cases. Assertions should
state behavior and include enough context to diagnose failure.

## Prove behavior, not just success

- Use temporary roots/homes and injected adapters. Do not let a test touch real
  packages, registry, services, user config, credentials, or the private overlay.
- Assert important command arguments, working directory, checked mode, and call
  count. Do not use an always-successful mock to hide unexpected commands.
- Model checked-command failure as `Err(ExecError)` and documented unchecked
  exit protocols as `ExecResult::failure`; also cover spawn/timeout/cancellation
  where relevant.
- For convergence, cover already-correct, missing, changed, blocked, dry-run,
  and repeat-run behavior. Assert no mutation calls or file changes in dry-run,
  not merely a success-shaped result.
- For parallel execution, assert required dependency order and outcomes, not
  incidental completion order. Prefer synchronization primitives to sleep-based
  tests.
- Add a regression case at the lowest layer that reproduces the bug, plus
  command-level coverage when selection, wiring, or exit status is affected.

## Snapshots

Update snapshots intentionally, review them, and commit them with the behavior
change. Do not accept broad rewrites without inspecting the diff.
Do not weaken assertions or regenerate unrelated snapshots just to make a
failure disappear.

## Scope validation

Test target selection and commands live in
[Testing](../../../docs/TESTING.md). CI topology belongs in `ci-cd-patterns`.
