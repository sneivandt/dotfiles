---
name: logging-patterns
description: >
  Logging conventions and patterns for the dotfiles Rust engine.
  Use when working with console output, task recording, or summary reporting.
---

# Logging Patterns

## Use this skill when

- changing console/log-file output behavior
- adding task result recording or summary behavior
- touching `cli/src/logging/` or logger usage in task execution

## Do not use this skill when

- changing scheduler/dependency behavior without logging changes (use
  `engine-orchestration`)
- changing generic error/idempotency policy (use `error-handling-patterns`)

## Invariants

- Initialize subscriber once at startup, then create one shared `Logger`.
- Access logging through `ctx.log`; do not construct additional loggers in tasks.
- Task result recording is owned by `tasks::execute()`; tasks should not call
  `record_task()` directly.
- Debug-level detail may be suppressed on terminal in non-verbose mode, but
  persistent logs remain complete.

Canonical implementations:
- `cli/src/logging/mod.rs`
- `cli/src/logging/subscriber.rs`
- `cli/src/logging/logger.rs`
- `cli/src/tasks/mod.rs` (task result recording path)

## Implementation procedure / core patterns

1. Pick message intent:
   - `debug`: per-item detail
   - `info`: concise progress/count detail
   - `warn` / `error`: visible problem signals
   - `dry_run`: explicit non-mutating preview
   - `always`: output that must always be user-visible
2. Keep task code focused on behavior; rely on buffered logging mechanics already
   provided by scheduler/task execution.
3. If changing summary semantics, verify completion-order task rows and final
   totals stay coherent in both verbose and non-verbose modes.

## Validation

- Run canonical local Rust/cross-platform validation from
  `cross-platform-verification`.
- Run focused tests for touched logging/scheduler areas.

## Common mistakes / anti-patterns

- Creating per-task logger instances
- Logging dry-run previews after mutation checks instead of before side effects
- Duplicating task recording in task implementations
- Re-implementing buffered output behavior in tasks
- Duplicating the canonical validation sequence in this skill

## Related skills

- `engine-orchestration`
- `error-handling-patterns`
- `testing-patterns`
