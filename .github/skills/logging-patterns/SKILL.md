---
name: logging-patterns
description: >
  Logging conventions and patterns for the dotfiles Rust engine.
  Use when working with console output, task recording, or summary reporting.
metadata:
  author: sneivandt
  version: "2.0"
---

# Logging Patterns

All logging is via `Logger` in `cli/src/logging.rs`, passed to tasks through `Context`.

## Logger API

```rust
impl Logger {
    pub fn stage(&self, msg: &str);     // Bold blue "==>" header
    pub fn info(&self, msg: &str);      // Indented message
    pub fn debug(&self, msg: &str);     // Only when verbose=true on terminal
    pub fn warn(&self, msg: &str);      // Yellow to stderr
    pub fn error(&self, msg: &str);     // Red to stderr
    pub fn dry_run(&self, msg: &str);   // Yellow "[DRY RUN]" prefix
}
```

All messages (including `debug`) are always written to a persistent log file at
`$XDG_CACHE_HOME/dotfiles/install.log` (default `~/.cache/dotfiles/install.log`)
with timestamps and ANSI codes stripped. The log file path is shown in the summary.

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
        ctx.log.record_task(task.name(), TaskStatus::Skipped, Some("not applicable"));
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
    let mut count = 0u32;
    for item in &ctx.config.items {
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
