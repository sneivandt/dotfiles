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
- Named `OutputExt` helpers are declared through the module-local
  `output_methods!` in [logging types](../../../cli/src/infra/logging/types.rs).
  Keep their docs and `MsgKind` mappings together; rendering and persistence
  policy belong in the sinks.
- `engine::execute()` records task results. Tasks do not call
  `record_task()` directly.
- Records are keyed by `Task::log_key()`, never display name.
- `ExecutionSummary` controls success and later phases. Logger counters are
  presentation only.
- Keep status labels/styles together in `logger/summary/status.rs::presentation`.
  Raw tracing and UI messages share formatting through `ui_line_with_style`;
  raw DEBUG/TRACE remain hidden even when UI debug details are verbose-visible.
- Reuse `duplicates_task_message` for buffered replay and completed rows, and
  `progress_clear_sequence` for cursor clearing. Do not duplicate these pure
  decisions across sinks.

## Output contract

Use [Console output](../../../docs/USAGE.md#console-output) for status words,
styles, spacing, visibility, and summary order. Use
[Execution output for scripts](../../../docs/USAGE.md#execution-output-for-scripts)
for quiet/JSON Lines behavior. The summary and subscriber test matrices specify
exact plain/ANSI rendering. Preserve these implementation constraints:

- Visible task rows stay in natural completion order.
- Not-applicable tasks stay out of console rows and final totals, but remain in
  persistent records and progress accounting. Task reasons belong on status
  rows; details must not restate them.
- Emit resource actions with `Output::action(verb, subject, planned, message)`.
  Supply the imperative console form in `message`, including contextual suffixes.
  Typed actions persist once before buffering. Sort only consecutive typed action
  runs and preserve warning/context barriers; never classify or rephrase messages
  by their text.
- Emit generated counters with `summary`, and mark generated task-result reasons
  with `with_summary_message`. Custom reasons remain visible even if they resemble
  counter text.
- Resource descriptions read `subject -> value`; symlinks are `target -> source`.
- Keep task and action counts independent; never derive totals from detail
  lines or parsed display text. Progress counts unfinished scheduled visible
  tasks within a phase; non-applicable tasks reduce it when they finish.
- Keep skipped, blocked, and interrupted outcomes distinct in rows and totals.
  Typed cancellation is interrupted; dependency prevention is blocked.
- Transient status lines are always cleared and never replace durable logging.

- Console filtering must preserve persistent records. JSON Lines must work
  when the persistent sink is unavailable and contain no human progress.
- A closed output pipe is benign and does not cancel convergence. Other write
  failures affect the exit code and retained outcome. Keep restart/force-quit
  completion on the same `Output::finish_run` path.

## Message intent

| Intent | Use |
|---|---|
| `trace` | plumbing that never reaches the console |
| `debug` / `context` | verbose diagnostic item detail / contextual block |
| `action` | applied or planned item detail with explicit verb and subject |
| `info` | informational detail, not an inferred action |
| `summary` | aggregate counters retained only in the run log |
| `warn` / `error` | visible problems |
| `dry_run` | planned mutation |
| `always` | output that must be visible |
| `startup` | startup notice |

## Persistent records

- Extend `records.rs::Record` for facts that consumers need to query. Keep schema
  versions explicit; preserve unknown records and legacy text in the viewer.
- Use stable task keys, not labels or `TypeId` debug text. Task decorators must
  forward `log_key()` and worker threads must inherit the owning task's context.
- Record run start, requested scope, resolved context, and finish independently of
  console totals. Each executed task has one readable start entry and one
  structured result/duration; do not add a task-stage header or text duplicates.
  Retain full error chains as debug diagnostics alongside the concise outcome.
  Restarted and elevated children receive the parent ID through `child_args`.
  A missing finish record means unfinished, not necessarily crashed.
- Failure hints select the exact run with `--id` and diagnostics with `-v`.
  Do not offer a hint when the persistent sink is unavailable or degraded.
- Preserve multiline messages and command streams in storage. Normal log viewing
  emphasizes actions/outcomes and includes checked command failures and nonempty
  stderr. Starts, resource checks, timings and debug details require `-v`.
  Unchecked nonzero exits use `nonzero_exit`, not a failure classification; retain
  their streams for verbose/raw diagnostics. `--raw` reads stored
  records; `--task` filters by stable public selector or exact stored identity.
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
