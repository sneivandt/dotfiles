---
description: >
  Rust coding conventions for the dotfiles engine. Use when writing or modifying
  Rust code in cli/src/: strict lints, error handling, macro-driven tasks,
  resource traits, and documentation style.
applyTo: "cli/src/**/*.rs"
---

# Rust Code Conventions

## Strict Lints

The project denies `panic`, `unwrap_used`, `expect_used`, `todo`, and `dbg_macro`.
Never use `.unwrap()` or `.expect()` — propagate with `?` using `anyhow::Result`,
or return typed errors from `cli/src/error.rs` (`ResourceError`, `ConfigError`).

## Error Handling

- Commands return `anyhow::Result<()>` — convert domain errors via `?`
- Resources return `Result<ResourceChange>` or `Result<ResourceState>`
- Use `ResourceError` factory methods (`not_found`, `command_failed`, etc.)
- See the `error-handling-patterns` skill for idempotency conventions

## Task Definition

Define tasks via the `resource_task!` macro in `cli/src/phases/`, not by
hand-implementing the `Task` trait. Use `task_deps!` for dependencies.
Register tasks in `cli/src/phases/catalog.rs`.

See the `resource-implementation` and `rust-patterns` skills for full templates.

## Resource Traits

Two traits exist in `cli/src/resources/mod.rs`:
- `Applicable` — describe, apply, remove (use when bulk state query needed)
- `Resource` — extends `Applicable` with `current_state()` (use when resource checks itself)

## Config Loading

Config modules in `cli/src/config/` use the `config_section!` macro.
Category filtering uses AND logic within groups. See the `toml-configuration` skill.

## Style

- `#[must_use]` on constructors and pure functions
- `const fn` where possible
- Derive `Debug` on all public types
- No trailing whitespace
