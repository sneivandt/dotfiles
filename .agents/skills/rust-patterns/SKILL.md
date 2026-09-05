---
name: rust-patterns
description: >
  Router for Rust changes under cli/src/. Use first when the task spans multiple
  engine layers or the owning subsystem is unclear. Do not use for config-only,
  shell-only, or already well-scoped domain changes.
---

# Rust Patterns

Use this as a routing map, not as a second source of subsystem rules.

## Find the owning code

Read the nearest implementation and tests before choosing a pattern. Domain
tasks live in `domains/<domain>/<feature>.rs`; shared `config/`, `resources/`,
and `tests/` directories have distinct roles. A feature support directory needs
its matching root entry point, not a generic `tasks/` folder.

Domains must not import the application or sibling domains. Shared adapters
belong in infrastructure; cross-domain composition belongs in the application.
Preserve the executable rules in
[`domain_boundaries`](../../../cli/tests/domain_boundaries.rs).

## Route the change

| Change | Load next |
|---|---|
| concrete `Resource`, state provider, resource task | `resource-implementation` |
| task graph, dependencies, `Operation`, `ProcessMode`, parallelism | `engine-orchestration` |
| errors, idempotency, dry-run result semantics | `error-handling-patterns` |
| console rows, details, progress, summaries | `logging-patterns` |
| TOML models/loaders | `toml-configuration` |
| semantic diagnostics or validation tasks | `config-validation` |
| packages, symlinks, profiles, overlays, APM | the corresponding domain skill |
| tests, fixtures, snapshots | `testing-patterns` |
| Windows-only code or behavior | `windows-specific-patterns` |
| local verification after Rust changes | `cross-platform-verification` |

Select the narrowest owner first; add companions only for contracts actually
changed. A shared word such as "state," "task," or "test" is not by itself a
reason to load every engine skill.

Repository-wide invariants live in
[AGENTS.md](../../../AGENTS.md), architecture in
[Architecture](../../../docs/ARCHITECTURE.md), and the change workflow in
[Contributing](../../../docs/CONTRIBUTING.md).
