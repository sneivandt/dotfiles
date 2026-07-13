---
name: rust-patterns
description: >
  Rust implementation map for the dotfiles core engine. Use when creating or
  modifying Rust code in cli/src/ to find the right focused skill and core
  project conventions.
---

# Rust Patterns

The Rust engine lives in `cli/` and owns all real behaviour: config parsing,
profile/platform resolution, resource planning, orchestration, logging, and CLI
commands. Shell wrappers only bootstrap and invoke the binary.

## Start Here

| Work area | Primary files | Use this skill |
|---|---|---|
| New or changed resource type | `cli/src/domains/<domain>/resources/`, `cli/src/domains/<domain>/tasks/` | `resource-implementation` |
| Operation-style task bodies, scheduling, dependencies, parallelism | `cli/src/engine/`, domain tasks, `cli/src/app/commands/` | `engine-orchestration` |
| Error handling, idempotency, dry-run behaviour | domain resources/tasks, `cli/src/engine/` | `error-handling-patterns` |
| Console output, task recording, summaries | `cli/src/runtime/logging/`, `cli/src/engine/task/execute.rs` | `logging-patterns` |
| TOML parsing or config sections | `cli/src/app/config/`, domain config modules, `conf/` | `toml-configuration`, `config-validation` |
| Profiles or sparse checkout | `cli/src/app/config/profiles.rs`, `cli/src/domains/repository/tasks/sparse_checkout/` | `profile-system`, `sparse-checkout-patterns` |
| Windows-specific features | registry, symlinks, PowerShell wrapper, platform gates | `windows-specific-patterns`, `cross-platform-verification` |
| Package installation | `cli/src/domains/packages/` | `package-management` |
| Overlay config or script tasks | `cli/src/domains/overlay/` | `overlay-scripts` |

## Project Layout

```text
cli/src/
├── app/            # CLI, commands, aggregate config, catalog, validation
├── domains/        # Vertical domains colocating config, resources, and tasks
├── engine/         # Generic task/resource/operation contracts and scheduling
├── runtime/        # Execution, filesystem, logging, platform, config support
└── testing/        # Feature-gated compatibility facade for integration tests
```

## Core Conventions

- Use `anyhow::Result` with contextual `?` propagation in commands/tasks, and
  typed `ResourceError` values in resource implementations when a resource-level
  failure needs classification.
- Prefer `config_resource_task!` for tasks with an injected
  `ConfigHandle<T>` and `resource_task!` for config-free resource tasks. For
  idempotent multi-step workflows that do not fit one resource, implement
  `Operation` and call `process_operation()` from the task body.
- Declare dependencies with `task_deps![...]`; register static tasks in
  `cli/src/app/catalog.rs`.
- Use `ExecutionPolicy` for central platform, dry-run, and elevation gates.
  Tasks that declare `RequiresElevation` must implement `needs_elevation()` so
  sudo is primed only when a privileged mutation is actually needed.
- Use capability methods such as `supports_systemd()`, `supports_chmod()`,
  `has_registry()`, `supports_aur()`, and `uses_pacman()` before direct OS checks.
- Route all subprocess calls through `ctx.executor`; do not call process helpers
  directly from tasks or resources.
- After self-update, the binary re-execs with `DOTFILES_REEXEC_GUARD` set to
  prevent infinite update loops; preserve this guard when changing update or
  wrapper handoff code.
- Public Rust items need `///` docs. Fallible public functions include
  `# Errors`; unsafe functions include `# Safety`.
- `cli/Cargo.toml` is the source of truth for strict lint policy. In addition
  to pedantic/nursery/cargo denies, code must avoid silent `as` conversions,
  ambiguous `Arc`/`Rc` `.clone()` calls, wildcard enum arms, unrelated
  shadowing, ignored `#[must_use]` values, and assertion messages without
  context.

## Task and Resource Rules

- Concrete tasks live under `cli/src/domains/<domain>/tasks/`; cross-domain
  validation tasks live in `cli/src/app/validation/`.
- Resource state should be discovered through `IntrinsicState` or a
  `ResourceStateProvider`, then applied through `process_resources()`,
  `process_resources_with_provider()`, or `process_resources_remove()`.
- Operation-style task bodies define an immutable `Operation::Plan`,
  return it through `OperationState::NeedsRun`, and consume that exact plan in
  `Operation::preview()` or `Operation::apply()`. Use `process_operation()` to
  centralize check -> dry-run -> mutate order.
- Fully custom tasks that cannot use resources or operations must still follow
  check -> dry-run -> mutate order manually.
- Inject typed `ConfigHandle<T>` values into config-backed tasks and keep read
  guards out of long-running or parallel work.
- Keep behaviour idempotent: re-running should converge to the same state.

## Validation

Use the canonical local sequence in `cross-platform-verification` for general
Rust/cross-platform checks. Keep this skill focused on routing and Rust
conventions.
