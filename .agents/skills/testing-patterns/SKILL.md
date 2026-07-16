---
name: testing-patterns
description: >
  Test construction and organization patterns for the dotfiles project.
  Use when adding or refactoring tests, snapshots, or test utilities.
---

# Testing Patterns

## Use this skill when

- adding or restructuring unit/integration tests
- deciding inline vs externalized `#[cfg(test)]` module layout
- using repo test helpers, snapshots, and fixture conventions

## Do not use this skill when

- selecting the canonical cross-platform validation command sequence (use
  `cross-platform-verification`)
- editing workflow topology or release pipeline behavior (use `ci-cd-patterns`)

## Decision guide

- Small cohesive tests: inline `#[cfg(test)] mod tests { ... }`
- Large cohesive tests: sibling `tests.rs` with `#[cfg(test)] mod tests;`
- Several related resource suites: one sibling `tests.rs`, with nested test
  modules when names or imports need separation.
- `#[path]` is allowed for externalized test modules only; never for production
  module wiring. Prefer standard sibling wiring for new layouts.

## Implementation procedure

1. Put unit tests with the module unless they are large enough to externalize.
2. Reuse existing helpers:
   - `cli/src/app/test_helpers.rs` context/config helpers for unit tests
   - `cli/tests/common/mod.rs` integration helpers and builders
3. Keep assertions specific and behavior-focused.
4. Update snapshots intentionally (`INSTA_UPDATE=unseen cargo test`, then
   `cargo insta review`).
5. Commit snapshot updates with the code change.

### Common module patterns

- **Config parsers**: use `infra::config::test_helpers`.
- **Resources**: construct directly for state checks; use executor-backed setup
  where needed.
- **Tasks**: test pure helper logic and task applicability/result behavior.
- **CLI parsing**: use `Cli::parse_from`.
- **Platform logic**: use `Platform::new(...)` test constructors.

## Validation

- For canonical local Rust/cross-platform validation, use
  `cross-platform-verification`.
- For CI-specific parity or workflow reproduction, use `ci-cd-patterns`.
- In this skill, run focused test commands for the area you changed (for example
  targeted `cargo test` by module/test target, snapshot review flow).

## Common mistakes / anti-patterns

- Using `#[path]` for production modules
- Rewriting snapshots without review
- Skipping task/helper conventions and rebuilding test scaffolding ad hoc
- Duplicating the canonical general validation command block instead of
  referencing `cross-platform-verification`

## Related skills

- `cross-platform-verification`
- `ci-cd-patterns`
- `module-organization`
- `resource-implementation`
