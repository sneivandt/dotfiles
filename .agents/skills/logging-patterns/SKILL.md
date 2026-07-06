---
name: logging-patterns
description: >
  Logging conventions and patterns for the dotfiles Rust engine.
  Use when working with console output, task recording, or summary reporting.
---

# Logging Patterns

All logging is via `Logger` in `cli/src/logging/`, passed to tasks through `Context`.
`Logger` emits [`tracing`](https://docs.rs/tracing) events internally; a
`DotfilesFormatter` subscriber (initialised in `main.rs`) formats them for the
console.

## Initialisation

Call `logging::init_subscriber(verbose, command)` **once** at program startup,
then create the `Logger` and sync its verbose flag:

```rust
// main.rs
logging::init_subscriber(args.verbose, command_name);
let mut log = logging::Logger::new(command_name);
log.set_verbose(args.verbose);
let log = Arc::new(log);
```

`set_verbose` updates both the `Logger` field and the global `AtomicBool` in
the subscriber module so that the console formatter and the logger stay in sync.

The subscriber routes `WARN`/`ERROR` to stderr and `INFO`/`DEBUG` to stdout.
When `verbose=false` the subscriber filters out `DEBUG` events on the terminal;
debug messages are **always** written to the log file regardless.

## Logger API

`ctx.log` is an `Arc<dyn Log>`. The `Log` trait is composed from two sub-traits:

- **`Output`** — user-facing display methods (`stage`, `info`, `debug`, `warn`,
  `error`, `dry_run`, `always`, `diagnostic`)
- **`TaskRecorder`** — structured task result recording (`record_task`)

`Log` is defined as `Log: Output + TaskRecorder` with a blanket implementation
(`impl<T: Output + TaskRecorder> Log for T {}`), so concrete types only implement
the two sub-traits.

Both `Logger` (top-level/direct output) and `BufferedLog` (scheduler-owned task
output) implement `Output` and `TaskRecorder`, and therefore automatically
implement `Log`.

**When to use each trait:**
- Accept `&dyn Log` (or `Arc<dyn Log>`) when you need both display and task
  recording (e.g., `Context`, `execute()`).
- Accept `&dyn Output` when you only need display methods and not task recording
  (e.g., `resolve_profile()`, `load_config()`).

```rust
ctx.log.stage(msg);       // Bold stage header on console; "==>" in main log
ctx.log.info(msg);        // Indented message (verbose-only on console)
ctx.log.debug(msg);       // Only when verbose=true on terminal
ctx.log.warn(msg);        // Yellow to stderr
ctx.log.error(msg);       // Red to stderr
ctx.log.dry_run(msg);     // Dry-run preview line
ctx.log.always(msg);      // Always visible on console AND log file
ctx.log.record_task(name, domain, status, message);  // Record task result for summary
ctx.log.diagnostic();     // Access high-precision diagnostic log (if available)
```

All messages (including `debug`) are always written to a persistent log file at
`$XDG_CACHE_HOME/dotfiles/<command>.log` (default `~/.cache/dotfiles/<command>.log`,
e.g. `install.log`, `uninstall.log`, `test.log`) with timestamps and ANSI codes
stripped. On Windows, when `XDG_CACHE_HOME` is not set, the path falls back to
`%USERPROFILE%\.cache\dotfiles\<command>.log`. The log file is always written
in verbose mode — all messages appear regardless of the console verbose flag.

| Method | Console (verbose) | Console (non-verbose) | Log file |
|--------|-------------------|-----------------------|----------|
| `stage` | Shown | Suppressed | Always |
| `info` | Shown | Suppressed | Always |
| `debug` | Shown | Suppressed | Always |
| `warn` | Shown | Shown | Always |
| `error` | Shown | Shown | Always |
| `dry_run` | Shown | Shown | Always |
| `always` | Shown | Shown | Always |

Task output is buffered by the scheduler. In non-verbose mode, buffered
`stage`, `info`, `debug`, `dry_run`, and `always` entries are replayed to the
main log only, then changed/skipped/failed/dry-run task results are printed to
the console as each task completes. `warn` and `error` entries remain
console-visible immediately when the task buffer flushes.

## Task Recording & Summary

`tasks::execute()` automatically records each task result:

```rust
pub fn execute(task: &dyn Task, ctx: &Context) {
    let domain = task.domain();
    if let Some(decision) = evaluate_policy(task, ctx) {
        record_policy_decision(ctx, task.name(), domain, decision);
        return;
    }
    if !task.should_run(ctx) {
        ctx.log.record_task(task.name(), domain, TaskStatus::NotApplicable, None);
        return;
    }
    match task.run_if_applicable(ctx) {
        Ok(Some(TaskResult::Ok)) => ctx.log.record_task(task.name(), domain, TaskStatus::Ok, None),
        Ok(None) => ctx.log.record_task(task.name(), domain, TaskStatus::NotApplicable, None),
        // ... Skipped, DryRun, Err handled similarly
    }
}
```

`log.print_summary()` shows only the final completion line and totals at end of
run. Changed/skipped/failed/dry-run task rows are emitted in completion order as
tasks finish; successful no-op and not-applicable tasks are counted but not shown
as rows. The persistent log file already contains each task's replayed
stage/detail output and receives the final totals. Don't call `record_task`
inside tasks.

## Pattern in Task::run()

```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    let items = ctx.config_read().items.clone();
    let mut count = 0u32;
    for item in &items {
        if ctx.dry_run {
            ctx.log.dry_run(&format!("would process {}", item.name));
            count += 1;
            continue;
        }
        ctx.log.debug(&format!("processing {}", item.name));
        count += 1;
    }
    if ctx.dry_run { return Ok(TaskResult::DryRun); }
    ctx.log.info(&format!("{count} items processed"));
    Ok(TaskResult::Ok)
}
```

## Verbose vs Non-Verbose Mode

When `verbose=false`:
- `stage`, `info`, and `debug` messages are suppressed on the console
- `warn`, `error`, `dry_run`, and `always` messages stay visible on the console
  for direct `Logger` output; task-buffered `dry_run`/`always` details are
  surfaced through the compact completed-task row
- Changed/skipped/failed/dry-run task rows are printed as tasks complete
- The final summary shows only `Complete`/`Failed`, elapsed time, and totals
- The progress line starts with `Running ·`, shows the first few task names plus
  a remaining count, and omits the trailing ellipsis

When `verbose=true`:
- All messages appear on the console as usual
- The final summary shows only `Complete`/`Failed`, elapsed time, and totals
- The progress line starts with `Running ·` and shows individual task names

The **log file** always receives full output regardless of the verbose setting.
The **`task_result`** target is internal to summary rendering and never written
to the log file. The **`file_only`** target is the inverse: written to the log
file but suppressed on the console.

## Rules

1. Access logger via `ctx.log` — never create a second `Logger`
2. Use `debug` for per-item detail; `info` for summary counts
3. Use `always` for structural output that must appear regardless of verbose mode
   (version, profile, summary totals)
4. Check `ctx.dry_run` before side effects; use `ctx.log.dry_run()` for preview
5. Return `TaskResult::DryRun` in dry-run mode
6. Task recording is automatic via `tasks::execute()` — don't call `record_task` in tasks

## Buffered Task Logging

Each scheduler-dispatched task receives a `BufferedLog` that captures output in
memory while the task runs. This is used in both parallel execution and the
`--no-parallel` sequential fallback so task headers/details render consistently.

- **On parallel task start**: `Logger::notify_task_start(name)` adds the task
  name to the active set and prints a `Running · task1, task2, ...` status line
- **On task complete**: `BufferedLog::flush_and_complete(name)` atomically
  replays all buffered entries (stage, info, debug, etc.) to the real Logger,
  emits the compact task result row when applicable, removes the task from the
  active set, and prints the updated status line
- **Flush lock**: A `Mutex<()>` on Logger serializes flushes so output from
  different tasks never interleaves
- **Task recording**: `record_task()` is forwarded immediately to the Logger
  (not buffered), since it's already thread-safe via its own Mutex

Tasks do **not** need to be aware of buffering — they log via `ctx.log` as
normal, and the `Log` trait dispatches to either `Logger` (sequential) or
`BufferedLog` (parallel) transparently.

## Diagnostic Log

A high-precision log written to `$XDG_CACHE_HOME/dotfiles/<command>.diag.log`
that captures every event **immediately** with microsecond wall-clock timestamps,
including real-time interleaving of parallel tasks (unlike the main log, which
replays buffered output per-task).

Each line: `<seq> +<elapsed_us> <wall_utc_us> [<context>] [<event>] <message>`

The event column records stable snake_case names like `[debug]`, `[task_done]`,
and `[resource_check]` without width padding after the bracket. The message
column records logger output, task scheduling state, and resource processing
details. Empty messages are omitted, and multiline messages are collapsed onto
one line so diagnostic logs do not contain blank rows.

Emit events via:

```rust
if let Some(diag) = ctx.log.diagnostic() {
    diag.emit(DiagEvent::ResourceCheck, "checking state");
    diag.emit_task(DiagEvent::TaskStart, "my task", "starting");
}
```

`BufferedLog` writes to the diagnostic log immediately (bypassing the buffer)
so parallel events have true timestamps. Both `Logger` and `BufferedLog`
implement `diagnostic()`.
