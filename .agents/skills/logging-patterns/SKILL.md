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
- Records are keyed by `Task::log_key()`, never display name.
- `ExecutionSummary` controls success and later phases. Logger counters are
  presentation only.
- Keep status labels/styles together in `logger/summary/status.rs::presentation`.
  Raw tracing and UI messages share formatting through `ui_line_with_style`;
  raw DEBUG/TRACE remain hidden even when UI debug details are verbose-visible.
- Reuse `is_redundant_detail` for buffered replay and completed rows, and
  `progress_clear_sequence` for cursor clearing. Do not duplicate these pure
  decisions across sinks.

## Output contract

- Visible task rows stay in natural completion order.
- Separate visible task blocks by one blank line in both verbosity modes;
  hidden tasks add no spacing. Keep details inside their task block.
- Task names and the first summary outcome are bold. Action details use normal
  contrast; context, reasons, and timing stay dim. `No changes` is bold in the
  default foreground. Plain output preserves spacing without ANSI styling.
- Non-verbose mode shows reportable outcomes; verbose mode also shows current
  and not-applicable tasks plus elapsed time.
- A task reason stays on its status row after ` · `. Indented lines are actions
  or planned actions and must not restate the row.
- Emit resource actions with `Output::action(verb, subject, planned, message)`.
  Typed actions persist once before buffering; legacy messages retain
  `compact_detail_line` as a fallback. Sort only consecutive action runs and
  preserve warning/context barriers.
- Resource descriptions read `subject -> value`; symlinks are `target -> source`.
- Final summaries count tasks, not detail lines or parsed display text.
- Progress uses `Running · {done}/{total} done · {active}`. Its denominator
  counts scheduled visible tasks within a phase. Non-applicable tasks advance
  progress but do not contribute to final totals.
- Keep skipped, blocked, and interrupted outcomes distinct in rows and totals.
  Typed cancellation is interrupted; dependency prevention is blocked.
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

## Persistent records

- Extend `records.rs::Record` for facts that consumers need to query. Keep schema
  versions explicit; preserve unknown records and legacy text in the viewer.
- Use stable task keys, not labels or `TypeId` debug text. Task decorators must
  forward `log_key()` and worker threads must inherit the owning task's context.
- Record run start, resolved context, and finish independently of console totals.
  Restarted and elevated children receive the parent ID through `child_args`.
  A missing finish record means unfinished, not necessarily crashed.
- Failure hints select the exact run with `--id` and diagnostics with `-v`.
  Do not offer a hint when the persistent sink is unavailable or degraded.
- Preserve multiline messages and command streams in storage. Normal log viewing
  includes failed command diagnostics and successful stderr. `--raw` reads stored
  records; `--task` filters by exact identity.
- Test lifecycle and parent linkage, stable selection after newer runs,
  task filtering, legacy/unknown records, output retention, and multiline parity.

Do not hardcode indentation, duplicate task recording, rephrase a task reason as
detail, or build task-local buffering.

Command arguments are logged by default. Use `CommandSpec::redact_arguments()`
when they contain sensitive values, and separately avoid exposing secrets in
stdout, stderr, resource descriptions, or errors. `OutputLog::Diagnostics`
retains failed streams and successful stderr; `Full` also retains successful
stdout. Use `OutputLog::Omit` for sensitive streams; it also removes streams
from checked-command errors. Successful and unchecked results remain available
to the caller, which must avoid logging them itself. Argument redaction is
independent. Use synthetic values in log fixtures and snapshots.

When summary semantics change, test statuses, visibility, details, progress
denominator, totals, both verbose modes, and `--no-symbols`. Preserve durable run
logs when changing console filtering or transient output. Start with
[summary rendering](../../../cli/src/infra/logging/logger/summary/) and
[subscriber tests](../../../cli/src/infra/logging/subscriber/tests.rs).
The summary and subscriber matrices own exact plain/ANSI presentation across
statuses, modes, verbosity, and symbols. [Buffered tests](../../../cli/src/infra/logging/buffered/tests.rs)
own direct/buffered persistence parity, completion order, and action barriers.
Wrapper style belongs in `shell-patterns`.
