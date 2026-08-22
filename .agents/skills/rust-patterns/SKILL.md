---
name: rust-patterns
description: >
  Router for Rust changes under cli/src/. Use first when the task spans multiple
  engine layers or the owning subsystem is unclear. Do not use for config-only,
  shell-only, or already well-scoped domain changes.
---

# Rust Patterns

Use this as a routing map, not as a second source of subsystem rules.

## Route the change

| Change | Load next |
|---|---|
| concrete `Resource`, state provider, resource task | `resource-implementation` |
| task graph, dependencies, `Operation`, `ProcessMode`, parallelism | `engine-orchestration` |
| errors, idempotency, dry-run result semantics | `error-handling-patterns` |
| console rows, details, progress, summaries | `logging-patterns` |
| TOML models/loaders or validation | `toml-configuration`, then `config-validation` if rules change |
| tests, fixtures, snapshots | `testing-patterns` |
| Windows-only code or behavior | `windows-specific-patterns` |
| local verification after Rust changes | `cross-platform-verification` |

Load only the rows touched by the task.

## Engine-wide invariants

- The Rust binary owns behavior; wrappers only bootstrap and forward.
- Keep domain code under `cli/src/domains/<domain>/`; generic contracts belong
  under `engine/` or `infra/`.
- Tasks own metadata, eligibility, dependencies, and orchestration boundaries.
  Resources own item-level convergence; operations own workflow convergence.
- Register static install/uninstall tasks in `cli/src/app/catalog.rs`.
- Read runtime environment through `ctx.env()` and run subprocesses through the
  context executor.
- Prefer platform capability methods over direct OS checks.
- Public fallible APIs document `# Errors`; every lint allowance includes a
  site-specific reason.

## Complete the slice

Trace the closest implementation before editing, then update every applicable
layer: config, loader, validation, resource or operation, task, exports,
registration, tests, and user documentation.

Use `cross-platform-verification` for the final command sequence.
