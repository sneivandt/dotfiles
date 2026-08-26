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

Repository-wide invariants live in
[AGENTS.md](../../../AGENTS.md), architecture in
[Architecture](../../../docs/ARCHITECTURE.md), and the change workflow in
[Contributing](../../../docs/CONTRIBUTING.md).
