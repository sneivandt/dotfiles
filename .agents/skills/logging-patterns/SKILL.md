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
- touching `cli/src/infra/logging/` or logger usage in task execution

## Do not use this skill when

- changing scheduler/dependency behavior without logging changes (use
  `engine-orchestration`)
- changing generic error/idempotency policy (use `error-handling-patterns`)

## Invariants

- Initialize subscriber once at startup, then create one shared `Logger`.
- Access logging through `ctx.log`; do not construct additional loggers in tasks.
- Task result recording is owned by `engine::execute()`; tasks should not call
  `record_task()` directly.
- Console output shows completion-order status rows for visible tasks with
  reportable outcomes. Internal, unchanged, and non-applicable tasks do not
  produce console rows; internal and non-applicable tasks remain excluded from
  visible totals, while unchanged tasks contribute to the aggregate `up to
  date` count. Task messages follow beneath emitted status rows with a two-space
  indent and dim styling.
- Statuses are fixed-width and explicit: `CHANGE`, `PASSED`, `DRYRUN`, `IGNORE`,
  and `FAILED`.
- Normal and verbose output both print all compact detail lines.
- Summaries count structured actions and affected tasks separately, plus ignored
  or failed tasks and elapsed time. Never infer counts from display text.
- Debug-level detail may be suppressed on terminal in non-verbose mode, but
  persistent logs remain complete.
- Header and summary lines join segments with ` · `. The shell wrappers
  deliberately mirror this style for bootstrap output so pre-binary and
  post-binary output look like one program; see `shell-patterns`.

Canonical implementations:
- `cli/src/infra/logging/mod.rs`
- `cli/src/infra/logging/subscriber/`
- `cli/src/infra/logging/logger/`
- `cli/src/engine/task/execute.rs` (task result recording)

## Implementation procedure / core patterns

1. Pick message intent:
   - `debug`: per-item detail
   - `info`: concise progress/count detail
   - `warn` / `error`: visible problem signals
   - `dry_run`: explicit non-mutating preview
   - `always`: output that must always be user-visible
2. Keep task code focused on behavior; rely on buffered logging mechanics already
   provided by scheduler/task execution.
3. If changing summary semantics, verify explicit statuses, visibility, compact
   details, completion-order rows, and action/task totals stay coherent in both
   verbose and non-verbose modes.

## Validation

Run focused tests for touched logging and scheduler behavior.

## Common mistakes / anti-patterns

- Creating per-task logger instances
- Logging dry-run previews after mutation checks instead of before side effects
- Duplicating task recording in task implementations
- Re-implementing buffered output behavior in tasks
