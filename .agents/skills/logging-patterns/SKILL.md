---
name: logging-patterns
description: >
  Use for changes under cli/src/infra/logging/, task result recording, status
  rows, progress lines, detail ordering, or final summaries. Not for generic
  error policy or task scheduling without output changes.
---

# Logging Patterns

## Ownership

- Startup initializes one shared `Logger`; task code uses `ctx.log()`.
- `engine::execute()` records task results. Tasks do not call
  `record_task()` directly.
- Records are keyed by `TaskId::record_key()`, never display name.
- `ExecutionSummary` controls success and later phases. Logger counters are
  presentation only.

## Output contract

- Visible task rows stay in natural completion order.
- Non-verbose mode shows reportable outcomes; verbose mode also shows current
  and not-applicable tasks plus elapsed time.
- A task reason stays on its status row after ` · `. Indented lines are actions
  or planned actions and must not restate the row.
- Normalize detail through `compact_detail_line`; sort only consecutive action
  runs inside one task.
- Resource descriptions read `subject -> value`; symlinks are `target -> source`.
- Final summaries count tasks, not detail lines or parsed display text.
- Progress uses `Running · {done}/{total} done · {active}` and the denominator
  matches visible summary accounting.
- Transient status lines are always cleared and never replace durable logging.

## Message intent

| Intent | Use |
|---|---|
| `trace` | plumbing that never reaches the console |
| `debug` | diagnostic item detail |
| `info` | concise action detail |
| `warn` / `error` | visible problems |
| `dry_run` | planned mutation |
| `always` | output that must be visible |
| `startup` | the single dim run-context header |

Do not hardcode indentation, duplicate task recording, rephrase a task reason as
detail, or build task-local buffering.

Command arguments are logged by default. Use `CommandSpec::redact_arguments()`
when they contain sensitive values, and separately avoid exposing secrets in
stdout, stderr, resource descriptions, or errors: argument redaction is not
output redaction. Use synthetic values in log fixtures and snapshots.

When summary semantics change, test statuses, visibility, details, progress
denominator, totals, both verbose modes, and `--no-symbols`. Preserve durable run
logs when changing console filtering or transient output. Start with
[summary rendering](../../../cli/src/infra/logging/logger/summary/) and
[subscriber tests](../../../cli/src/infra/logging/subscriber/tests.rs).
Wrapper style belongs in `shell-patterns`.
