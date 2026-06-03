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
| New or changed resource type | `cli/src/resources/`, `cli/src/tasks/<domain>/` | `resource-implementation` |
| Task scheduling, dependencies, parallelism | `cli/src/engine/`, `cli/src/commands/mod.rs` | `engine-orchestration` |
| Error handling, idempotency, dry-run behaviour | `cli/src/resources/`, `cli/src/tasks/` | `error-handling-patterns` |
| Console output, task recording, summaries | `cli/src/logging/` | `logging-patterns` |
| TOML parsing or config sections | `cli/src/config/`, `conf/` | `toml-configuration`, `config-validation` |
| Profiles or sparse checkout | `cli/src/config/profiles.rs`, `cli/src/tasks/repository/sparse_checkout/mod.rs` | `profile-system`, `sparse-checkout-patterns` |
| Windows-specific features | registry, symlinks, PowerShell wrapper, platform gates | `windows-specific-patterns`, `cross-platform-verification` |
| Package installation | `cli/src/resources/package.rs`, `cli/src/tasks/packages/mod.rs` | `package-management` |
| Overlay config or script tasks | `cli/src/config/overlay.rs`, `cli/src/resources/script.rs` | `overlay-scripts` |

## Project Layout

```text
cli/src/
├── main.rs         # Entry point: logging setup and command dispatch
├── cli.rs          # clap CLI definitions
├── config/         # TOML loading, profile/category filtering, validation
├── resources/      # Declarative Resource, IntrinsicState, providers
├── engine/         # Context, resource plans, orchestration, scheduler
├── tasks/          # Task trait, macros, task catalog, domain-grouped tasks
├── commands/       # install, uninstall, test, version, logs command runners
├── logging/        # Logger, buffered parallel output, diagnostic logs
├── exec.rs         # Executor abstraction for subprocesses
└── platform.rs     # OS/capability detection
```

## Core Conventions

- Use `anyhow::Result` with contextual `?` propagation in commands/tasks, and
  typed `ResourceError` values in resource implementations when a resource-level
  failure needs classification.
- Prefer the `resource_task!` macro for config-to-resource tasks; use manual
  `Task` implementations only for non-standard orchestration or dynamic tasks.
- Declare dependencies with `task_deps![...]`; register static tasks in
  `cli/src/tasks/catalog.rs`.
- Use `ExecutionPolicy` for central platform, dry-run, and elevation gates.
  Tasks that declare `RequiresElevation` must implement `needs_elevation()` so
  sudo is primed only when a privileged mutation is actually needed.
- Use capability methods such as `supports_systemd()`, `supports_chmod()`,
  `has_registry()`, `supports_aur()`, and `uses_pacman()` before direct OS checks.
- Route all subprocess calls through `ctx.executor`; do not call process helpers
  directly from tasks or resources.
- Public Rust items need `///` docs. Fallible public functions include
  `# Errors`; unsafe functions include `# Safety`.

## Task and Resource Rules

- Tasks live in domain folders under `cli/src/tasks/<domain>/` or
  `cli/src/tasks/validation/mod.rs`.
- Resource state should be discovered through `IntrinsicState` or a
  `ResourceStateProvider`, then applied through `process_resources()`,
  `process_resources_with_provider()`, or `process_resources_remove()`.
- Custom non-resource tasks must follow check -> dry-run -> mutate order.
- Clone config data out of `ctx.config_read()` before long-running work or
  parallel processing.
- Keep behaviour idempotent: re-running should converge to the same state.

## Validation

For Rust changes, run the local checks described in `cross-platform-verification`:

```sh
cd cli
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test
```

If the Windows target/toolchain is unavailable, say so explicitly in the final
handoff instead of silently skipping it.
