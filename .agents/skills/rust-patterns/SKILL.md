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
| New or changed resource type | `cli/src/domains/<domain>/resources/`, `cli/src/domains/<domain>/<task>.rs` | `resource-implementation` |
| Operation-style task bodies, scheduling, dependencies, parallelism | `cli/src/engine/`, domain tasks, `cli/src/app/commands/` | `engine-orchestration` |
| Error handling, idempotency, dry-run behaviour | domain resources/tasks, `cli/src/engine/` | `error-handling-patterns` |
| Console output, task recording, summaries | `cli/src/infra/logging/`, `cli/src/engine/task/execute.rs` | `logging-patterns` |
| TOML parsing or config sections | `cli/src/app/config/`, domain config modules, `conf/` | `toml-configuration`, `config-validation` |
| Profiles or sparse checkout | `cli/src/app/config/profiles.rs`, `cli/src/domains/repository/sparse_checkout.rs`, `cli/src/domains/repository/sparse_checkout/` | `profile-system`, `sparse-checkout-patterns` |
| Windows-specific features | registry, symlinks, PowerShell wrapper, platform gates | `windows-specific-patterns`, `cross-platform-verification` |
| Package installation | `cli/src/domains/packages/` | `package-management` |
| Overlay config or script tasks | `cli/src/domains/overlay/` | `overlay-scripts` |

## Project Layout

```text
cli/src/
├── app/            # CLI, commands, aggregate config, catalog, validation
├── domains/        # Vertical domains colocating config, resources, and tasks
├── engine/         # Generic task/resource/operation contracts and scheduling
├── infra/          # Execution, filesystem, logging, platform, config support
└── testing/        # Feature-gated compatibility facade for integration tests
```

## Module Layout Conventions

- Use the standard Rust module layout. `mod.rs` handles wiring and public
  re-exports; keep implementation logic in focused sibling files.
- `config/` and `resources/` are the shared domain subdirectory categories, and
  `tests/` holds externalized domain tests. A large feature may use
  `<feature>.rs` as its root entry with support modules under `<feature>/`.
- Split a module by domain responsibility rather than by size alone.

## Core Conventions

- Use `anyhow::Result` with contextual `?` propagation in commands/tasks, and
  typed `ResourceError` values in resource implementations when a resource-level
  failure needs classification.
- Implement `Task` directly; there is no task-generating macro. Use
  `task_metadata!` for the metadata block, `task_deps![...]` for dependencies,
  and call `run_resource_task()` (or `run_batch_resource_task()` when one query
  feeds every resource) from the task body. Config-backed tasks own a
  `ConfigHandle<T>` field and snapshot it with `self.config.read().to_vec()`.
  Implement `run_configured()` and `run()` as the two-line pair that passes
  `Some(NAME)` / `None` as the stage announcement. For idempotent multi-step
  workflows that do not fit one resource, implement `Operation` and call
  `process_operation()` from the task body.
- Declare dependencies with `task_deps![...]`; register static tasks in
  `cli/src/app/catalog.rs`.
- Ordering comes only from dependencies; catalog insertion order is arbitrary.
  Mark a task with `update_only: true` only when it belongs to `dotfiles update`
  but not `dotfiles install`.
- Use `should_run()` for platform, tool-availability, and configuration
  eligibility. Implement `needs_elevation()` only when an applicable task's
  current state predicts a privileged mutation, so elevation is requested only
  when needed. Dry-run suppression is handled centrally by the
  `engine::requires_elevation` free function, which decorators cannot bypass.
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
- `allow_attributes_without_reason` is denied, so every `#[allow(...)]` carries
  a `reason`. Make it say why *this* site is safe (the invariant, the platform
  gate, the trait shape), not merely that the lint was disabled; do not park the
  justification in an adjacent comment.

## Task and Resource Rules

- Concrete task entry modules live directly under
  `cli/src/domains/<domain>/`; large features may keep supporting modules in a
  same-named subdirectory. Cross-domain validation tasks live in
  `cli/src/app/validation/`.
- Resource state should be discovered through `IntrinsicState` or a
  `ResourceStateProvider`, then applied through `process_resources()`,
  `process_resources_with_provider()`, or `process_resources_remove()`.
- Operation-style task bodies define an immutable `Operation::Plan`,
  return it through `OperationState::NeedsRun`, and consume that exact plan in
  `Operation::preview()` or `Operation::apply()`. Use `process_operation()` to
  centralize check -> dry-run -> mutate order.
- Fully custom tasks that cannot use resources or operations must still follow
  check -> dry-run -> mutate order manually.
- Keep applicability centralized: `should_run()` decides eligibility, while
  `run_configured()` only suppresses tasks with no configured work.
- Keep task metadata roles distinct; `engine-orchestration` owns the full
  `TaskId`/`selector()`/`name()`/`visibility()` statement.
- Preserve discovery insertion order and natural parallel completion-order
  output; do not sort completed task rows.
- Inject typed `ConfigHandle<T>` values into config-backed tasks and keep read
  guards out of long-running or parallel work.
- Keep behaviour idempotent: re-running should converge to the same state.
