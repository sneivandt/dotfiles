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
- Console output shows completion-order status rows for visible tasks. Non-verbose
  mode emits rows only for tasks with a reportable outcome; verbose mode accounts
  for every visible task and appends per-task elapsed time. Internal tasks never
  produce console rows and stay out of every total.
- Statuses use a one-cell glyph column: `✓` for changed or passed, `~` for dry
  run, `⊘` for skipped, `✗` for failed, and verbose-only `‧` for up to date and
  `⁃` for not applicable. The glyphs are ASCII or Neutral width, contain no
  variation selectors, and therefore align reliably with `chars().count()`.
  `--no-symbols` restores the ASCII word tokens for terminals or pipelines that
  cannot render them. `⁃` rows carry no elapsed time because nothing ran.
- A task's reason belongs on its status row after a ` · ` separator, not on an
  indented line. Indented, dim, two-space lines beneath a row are therefore
  always actions the task took or planned. A blank line separates a task that
  printed actions from the row that follows it.
- Detail lines are normalized identically in both modes through
  `compact_detail_line`, so a given action reads the same whether or not
  `--verbose` is set. Maximal runs of consecutive action lines within a task are
  sorted so parallel work produces stable output; any non-action line acts as a
  barrier and preserves surrounding order. This applies to detail lines only —
  completed task rows keep their natural completion order.
- Resource descriptions read left to right as `subject \u{2192} value`. Symlinks
  render as `target \u{2192} source`; never reverse the arrow for one resource.
- Never emit a line that restates what the status row already says. Aggregate
  counter summaries and lines duplicating the task message are filtered in both
  modes; do not work around the filter by rewording.
- Normal and verbose output both print all detail lines, uncapped.
- Summaries count structured actions and affected tasks separately, plus
  up-to-date, ignored, and failed tasks and elapsed time. Non-applicable tasks
  are reported nowhere but the run log. Never infer counts from display text.
- The transient progress line reads
  `Running · {done}/{total} done · {active tasks}`. The counter is completed
  tasks, not active ones, so it keeps its explicit `done` label. Its denominator
  counts exactly the visible tasks the summary accounts for, so
  changing what the summary reports means changing the denominator too.
  Applicability is only known after a task runs, so a non-applicable task leaves
  the denominator rather than advancing the numerator, and totals accumulate
  across scheduled graphs because late-discovered tasks join a second graph
  mid-run.
- `status_line` / `clear_status_line` draw a transient line for pre-scheduler
  work that would otherwise be silent, such as the self-update check. The line
  must always be cleared, and it must not be the only record of an event:
  anything worth remembering also needs a durable line. Self-update prints one
  persistent line only when a new version was actually installed.
- Debug-level detail may be suppressed on terminal in non-verbose mode, but
  persistent logs remain complete. `MsgKind::Trace` goes one step further and
  never reaches the console, even under `--verbose`; use it for plumbing chatter
  such as parallelism and batching counts.
- Buffered logging recovers poisoned entry locks rather than dropping later
  diagnostics. A task panic must not make the remaining failure context vanish.
- Header and summary lines join segments with ` · `. The shell wrappers
  deliberately mirror this style for bootstrap output so pre-binary and
  post-binary output look like one program; see `shell-patterns`.
- The startup header is a single line: command, profile, platform, then the
  optional `dry run` and `overlay <path>` sections. It is emitted with
  `MsgKind::Startup` and rendered dim so it reads as run context rather than a
  result. Never add a second startup line.

Canonical implementations:
- `cli/src/infra/logging/mod.rs`
- `cli/src/infra/logging/subscriber/`
- `cli/src/infra/logging/logger/`
- `cli/src/engine/task/execute.rs` (task result recording)

## Implementation procedure / core patterns

1. Pick message intent:
   - `trace`: plumbing chatter that stays out of the console entirely
   - `debug`: per-item detail
   - `info`: concise progress/count detail
   - `warn` / `error`: visible problem signals
   - `dry_run`: explicit non-mutating dry-run action
   - `always`: output that must always be user-visible
   - `startup`: the dim run-context header; emitted once by the command runner
2. Keep task code focused on behavior; rely on buffered logging mechanics already
   provided by scheduler/task execution.
3. If changing summary semantics, verify explicit statuses, visibility, detail
   lines, completion-order rows, progress denominator, and action/task totals
   stay coherent in both verbose and non-verbose modes.

## Validation

Run focused tests for touched logging and scheduler behavior.

## Common mistakes / anti-patterns

- Creating per-task logger instances
- Logging dry-run actions after mutation checks instead of before side effects
- Restating a task's reason on an indented line below its status row. The
  automatic filter only removes exact matches (and a few known failure
  prefixes), so a reworded restatement such as `installed: 19 APM dependencies`
  under a row reading `installed 19 APM dependencies` slips through. When a
  task both returns a reason and wants the phrase in the run log, emit it with
  `trace`, not `info` or `always`.
- Duplicating task recording in task implementations
- Re-implementing buffered output behavior in tasks
- Hardcoding indentation into a message instead of choosing the right intent
- Leaving a status line on screen, or using one as the only record of an event
