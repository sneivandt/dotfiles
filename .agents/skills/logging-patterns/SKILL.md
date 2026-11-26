---
name: logging-patterns
description: >
  Use for changes under cli/src/infra/logging/, dotfiles log rendering,
  console colors/spacing, task result recording, status rows, progress lines,
  detail ordering, final summaries, or updating
  docs/public/assets/terminal-screenshot.svg. Not for generic error policy or
  task scheduling without output changes.
---

# Logging Patterns

User-facing console and JSON Lines contracts live in
[Console output](../../../docs/content/guides/commands-and-usage.md#console-output)
and [Execution output for scripts](../../../docs/content/reference/cli-reference.md#execution-output-for-scripts);
exact `MsgKind` helpers live in `output_methods!` in
[types.rs](../../../cli/src/infra/logging/types.rs).

## Ownership and shared rendering

- `engine::execute()` calls `record_task()` keyed by `Task::log_key()` (never
  display name); tasks do not record themselves or build task-local buffering.
- `ExecutionSummary` controls run success and later phases; logger counters are
  presentation only.
- Keep status styles in `logger/summary/status.rs::presentation`, raw/UI line
  formatting in `ui_line_with_style` (raw `DEBUG`/`TRACE` stay hidden even when
  UI debug details are visible), replay deduplication in
  `duplicates_task_message`, and cursor clearing in `progress_clear_sequence`.

## Console and action invariants

- Visible task rows appear in natural completion order. `NotApplicable` tasks
  stay out of console rows and final totals, but remain in persistent records
  and reduce the progress denominator when they finish. Task reasons belong on
  status rows, never restated as detail lines.
- Emit resource actions via `Output::action(verb, subject, planned, message)`
  (persisted once before buffering). Sort only consecutive typed action runs
  across warning/context barriers; never classify or rephrase by text. Keep
  `subject` opaque for parentheticals/current-state text (dimming only the
  structural ` → ` arrow and identifiable trailing suffixes outside the subject).
- Mark generated counter reasons with `with_summary_message` so custom reasons
  stay visible even if they resemble counters.
- Hold the shared console lock through transient status/progress redraw and
  ticker shutdown. Fit plain text before styling and reserve the autowrap
  column; `NO_COLOR` disables styling, not cursor control. Task-block spacing
  depends only on actual visible details in the active verbosity mode.
- Keep skipped, blocked (dependency prevention), and interrupted (typed
  cancellation) distinct. Console filtering must not drop persistent records;
  JSON Lines must work when the persistent sink is unavailable.
- A closed output pipe is benign and does not cancel convergence; other write
  failures affect exit code and retained outcome. Keep restart/force-quit
  completion on `Output::finish_run`.

## Persistent records

- Extend `records.rs::Record` with explicit schema versions and preserve
  unknown/legacy records in the viewer. Worker threads must inherit task log
  context; restarted/elevated children receive parent ID via `child_args`.
- Retain full error chains as debug diagnostics alongside concise outcomes.
  Unchecked nonzero exits use `nonzero_exit` (not failure classification) with
  streams retained for verbose/raw viewing. A missing finish record means
  unfinished, not necessarily crashed.
- Failure recovery hints must target the exact run (`--id`) and `-v`, and must
  be omitted when the persistent sink is unavailable or degraded.

## Coupled assets and test matrices

When user-visible `dotfiles update` output changes, check whether
[terminal-screenshot.svg](../../../docs/public/assets/terminal-screenshot.svg)
and [site.spec.ts](../../../docs/tests/browser/site.spec.ts) need updating.
Exact plain/ANSI matrices live in
[summary](../../../cli/src/infra/logging/logger/summary/),
[subscriber/tests.rs](../../../cli/src/infra/logging/subscriber/tests.rs), and
[buffered/tests.rs](../../../cli/src/infra/logging/buffered/tests.rs).
Wrapper output style belongs in [self-update](../self-update/SKILL.md#wrapper-bootstrap).
