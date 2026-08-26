---
name: testing-patterns
description: >
  Use when adding or restructuring Rust tests, fixtures, mocks, snapshots, or
  test utilities in cli/. Not for choosing CI jobs or the general validation
  command sequence.
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
- Ordered fixed command scripts: `ScriptedExecutor`
- Branching or unordered execution: `MockExecutor`
- Common task assertions: `assert_task_ok`, `assert_task_changed`,
  `task_batch`, `task_skipped`
- APM setup: `domains/ai/apm/test_fixture.rs`
- Environment behavior: inject `MapEnv`; never mutate process-global env

Prefer named case tables for repeated input/output cases. Assertions should
state behavior and include enough context to diagnose failure.

## Snapshots

Update snapshots intentionally, review them, and commit them with the behavior
change. Do not accept broad rewrites without inspecting the diff.

## Scope validation

Test target selection and commands live in
[Testing](../../../docs/TESTING.md). CI topology belongs in `ci-cd-patterns`.
