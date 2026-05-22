---
description: >
  Rust coding conventions for the dotfiles engine. Use when writing or modifying
  Rust code in cli/src/: strict lints, error handling, macro-driven tasks,
  resource traits, and documentation style.
applyTo: "cli/src/**/*.rs"
---

# Rust Code Conventions

## Strict Lints

The project denies `panic`, `unwrap_used`, `expect_used`, `todo`, `dbg_macro`,
`arithmetic_side_effects`, `let_underscore_drop`, `unused_result_ok`,
`allow_attributes_without_reason`, and `unreachable_pub` (among others — see
`cli/Cargo.toml` for the full list).

Never use `.unwrap()` or `.expect()` — propagate with `?` using `anyhow::Result`,
or return typed errors from `cli/src/error.rs` (`ResourceError`, `ConfigError`).

Every `#[allow(...)]` must include a `reason = "..."` argument. Never use bare
`let _ = expr;` — see the `error-handling-patterns` skill.

## Error Handling

- Commands return `anyhow::Result<()>` — convert domain errors via `?`
- Resources return `Result<ResourceChange>` or `Result<ResourceState>`
- Use `ResourceError` factory methods (`not_found`, `command_failed`, etc.)
- See the `error-handling-patterns` skill for idempotency conventions

## Task Definition

Define tasks via the `resource_task!` macro in `cli/src/phases/`, not by
hand-implementing the `Task` trait. Use `task_deps!` for dependencies.
Register tasks in `cli/src/phases/catalog.rs`.

Use `ExecutionPolicy` for orchestration-level gates: platform support,
dry-run-only skips, and elevation declarations. For tasks declaring
`RequiresElevation`, implement `needs_elevation()` so sudo is primed only when
the task is applicable and a privileged mutation is predicted.

See the `resource-implementation` and `rust-patterns` skills for full templates.

## Resource Traits

Two traits exist in `cli/src/resources/mod.rs`:
- `Applicable` — describe, apply, remove (use when bulk state query needed)
- `Resource` — extends `Applicable` with `current_state()` (use when resource checks itself)

## Config Loading

Config modules in `cli/src/config/` use the `config_section!` macro.
Category filtering uses AND logic within groups. See the `toml-configuration` skill.

## Documentation

All public items require `///` doc comments. Sections in order:

1. **Main description** — brief summary (no header)
2. **`# Examples`** — compiled as doctests unless annotated `ignore`/`bash`/`text`
3. **`# Errors`** — **required** for every function returning `Result<T>`
4. **`# Panics`** — document any panic conditions
5. **`# Safety`** — required for `unsafe` functions

Document all public fields, enum variants, and trait methods.

## Style

- `#[must_use]` on constructors, boolean queries (`is_*`, `has_*`, `supports_*`), and pure functions
- `const fn` where possible
- Derive `Debug` on all public types
- No trailing whitespace
