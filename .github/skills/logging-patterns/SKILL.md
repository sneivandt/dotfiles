---
name: logging-patterns
description: >
  Logging conventions and patterns for the dotfiles Rust engine.
  Use when working with console output, task recording, or summary reporting.
metadata:
  author: sneivandt
  version: "3.0"
---

# Logging Patterns

All logging is via `Logger` in `cli/src/logging/`, passed to tasks through `Context`.
`Logger` emits [`tracing`](https://docs.rs/tracing) events internally; a
`DotfilesFormatter` subscriber (initialised in `main.rs`) formats them for the
console.

## Initialisation

Call `logging::init_subscriber(verbose)` **once** at program startup (before
creating the `Logger`) to register the global tracing subscriber:

```rust
// main.rs
logging::init_subscriber(args.verbose);
let log = Arc::new(logging::Logger::new(command_name));
```

The subscriber routes `WARN`/`ERROR` to stderr and `INFO`/`DEBUG` to stdout.
When `verbose=false` the subscriber filters out `DEBUG` events on the terminal;
debug messages are **always** written to the log file regardless.

## Logger API

`ctx.log` is an `Arc<dyn Log>` — both `Logger` (sequential) and `BufferedLog`
(parallel) implement the `Log` trait. All logging goes through these methods:

```rust
ctx.log.stage(msg);    // Bold blue "==>" header
ctx.log.info(msg);     // Indented message
ctx.log.debug(msg);    // Only when verbose=true on terminal
ctx.log.warn(msg);     // Yellow to stderr
ctx.log.error(msg);    // Red to stderr
ctx.log.dry_run(msg);  // Yellow "[DRY RUN]" prefix
ctx.log.record_task(name, status, message);  // Record task result for summary
ctx.log.diagnostic();  // Access high-precision diagnostic log (if available)
```

All messages (including `debug`) are always written to a persistent log file at
`$XDG_CACHE_HOME/dotfiles/<command>.log` (default `~/.cache/dotfiles/<command>.log`,
e.g. `install.log`, `uninstall.log`, `test.log`) with timestamps and ANSI codes
stripped.  The `debug` method only prints to the terminal when `verbose=true`,
but **always** writes to the log file regardless of the verbose flag.
The log file path is shown in the summary.

| Method | Use For |
|--------|---------|
| `stage` | Major section headers (one per task) |
| `info` | Summary counts ("12 symlinks created") |
| `debug` | Per-item detail (verbose only on terminal, always in log file) |
| `dry_run` | Preview of what would happen |

## Task Recording & Summary

`tasks::execute()` automatically records each task result:

```rust
pub fn execute(task: &dyn Task, ctx: &Context) {
    if !task.should_run(ctx) {
        ctx.log.debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log.record_task(task.name(), TaskStatus::NotApplicable, None);
        return;
    }
    ctx.log.stage(task.name());
    match task.run(ctx) {
        Ok(TaskResult::Ok) => ctx.log.record_task(task.name(), TaskStatus::Ok, None),
        // ... Skipped, DryRun, Err handled similarly
    }
}
```

`log.print_summary()` shows totals at end of run. Don't call `record_task` inside tasks.

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

## Rules

1. Access logger via `ctx.log` — never create a second `Logger`
2. Use `debug` for per-item detail; `info` for summary counts
3. Check `ctx.dry_run` before side effects; use `ctx.log.dry_run()` for preview
4. Return `TaskResult::DryRun` in dry-run mode
5. Task recording is automatic via `tasks::execute()` — don't call `record_task` in tasks

## Parallel Task Logging

When parallel execution is enabled, each task receives a `BufferedLog` that
captures output in memory while the task runs.

- **On task start**: `Logger::notify_task_start(name)` adds the task name to
  the active set and prints a dim status line (`▹ task1, task2, ...`)
- **On task complete**: `BufferedLog::flush_and_complete(name)` atomically
  replays all buffered entries (stage, info, debug, etc.) to the real Logger,
  removes the task from the active set, and prints the updated status line
- **Flush lock**: A `Mutex<()>` on Logger serializes flushes so output from
  different tasks never interleaves
- **Task recording**: `record_task()` is forwarded immediately to the Logger
  (not buffered), since it's already thread-safe via its own Mutex

Tasks do **not** need to be aware of buffering — they log via `ctx.log` as
normal, and the `Log` trait dispatches to either `Logger` (sequential) or
`BufferedLog` (parallel) transparently.

## Diagnostic Log

A high-precision diagnostic log captures the **real-time** sequence of all
events — including parallel execution — at microsecond resolution.  Unlike
the main log file (which replays buffered output per-task), the diagnostic
log writes every event **immediately** with the true wall-clock time.

Written to `$XDG_CACHE_HOME/dotfiles/<command>.diag.log` (shown in the
summary alongside the main log path).

### Line Format

```
+<elapsed_us> <wall_utc_us> [<thread>] <TAG>         <message>
```

Example:
```
+       123 2026-02-25T10:30:00.000123Z [main]     STAGE        Resolving profile
+      5678 2026-02-25T10:30:00.005678Z [thread-2] TASK_WAIT    [Install symlinks] waiting for: Update repository
+     50123 2026-02-25T10:30:00.050123Z [thread-3] RES_CHECK    ~/.bashrc state=Missing
+     51456 2026-02-25T10:30:00.051456Z [thread-3] RES_APPLY    link ~/.bashrc
+     52789 2026-02-25T10:30:00.052789Z [thread-3] RES_RESULT   ~/.bashrc applied
```

### Event Tags

| Tag | Origin | Meaning |
|---|---|---|
| `STAGE` | Logger/BufferedLog | Major section header |
| `INFO` | Logger/BufferedLog | Informational message |
| `DEBUG` | Logger/BufferedLog | Debug detail |
| `WARN` | Logger/BufferedLog | Warning |
| `ERROR` | Logger/BufferedLog | Error |
| `DRYRUN` | Logger/BufferedLog | Dry-run preview |
| `TASK_WAIT` | Parallel scheduler | Task spawned, waiting for deps |
| `TASK_START` | Parallel scheduler | Deps satisfied, execution begins |
| `TASK_DONE` | Parallel scheduler | Task finished |
| `TASK_SKIP` | Task execute | Task skipped (not applicable) |
| `RES_CHECK` | `process_single()` | Resource state checked |
| `RES_APPLY` | `apply_resource()` | Resource mutation started |
| `RES_RESULT` | `apply_resource()` | Resource mutation result |
| `RES_REMOVE` | `remove_single()` | Resource removal |

### Accessing the Diagnostic Log

The `Log` trait exposes `diagnostic()` which returns `Option<&DiagnosticLog>`.
Both `Logger` and `BufferedLog` implement this (BufferedLog delegates to its
inner Logger).

```rust
if let Some(diag) = ctx.log.diagnostic() {
    diag.emit(DiagEvent::ResourceCheck, "checking state");
    diag.emit_task(DiagEvent::TaskStart, "my task", "starting");
}
```

### Key Design Points

- `BufferedLog` writes to the diagnostic log **immediately** (bypassing the
  buffer) so parallel events have true timestamps
- All display methods (`stage`, `info`, `debug`, etc.) emit to both the
  tracing subscriber (for console + main log) and the diagnostic log
- The diagnostic log file is created by `Logger::new()` alongside the main log
- Thread names from `std::thread::current().name()` identify parallel threads
