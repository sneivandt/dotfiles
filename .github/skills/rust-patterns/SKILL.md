---
name: rust-patterns
description: >
  Rust coding patterns and conventions for the dotfiles core engine.
  Use when creating or modifying Rust code in cli/src/.
metadata:
  author: sneivandt
  version: "1.0"
---

# Rust Patterns

The core engine is a single Rust crate in `cli/` using `anyhow` for errors and `clap` (derive) for CLI.

## Project Layout

```
cli/src/
├── main.rs        # Entry: parse CLI, dispatch commands
├── cli.rs         # clap CLI definitions (Cli, Command, GlobalOpts)
├── platform.rs    # OS detection (Platform, Os enum)
├── exec.rs        # Command execution helpers
├── logging.rs     # Logger with leveled output + task summary
├── config/        # INI parsing and config loading
│   ├── mod.rs     # Config struct (aggregates all types)
│   ├── ini.rs     # Section/KvSection parsers, filter_sections_and()
│   ├── profiles.rs
│   └── *.rs       # Per-type loaders (packages, symlinks, etc.)
├── tasks/         # Task implementations
│   ├── mod.rs     # Task trait, Context struct, execute()
│   └── *.rs       # One file per task
└── commands/      # install.rs, uninstall.rs, test.rs
```

## The Task Trait

```rust
pub trait Task {
    fn name(&self) -> &str;
    fn should_run(&self, ctx: &Context) -> bool;
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}
pub enum TaskResult { Ok, Skipped(String), DryRun }
```

### New Task Template

```rust
pub struct MyTask;
impl Task for MyTask {
    fn name(&self) -> &str { "My task" }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !ctx.config.items.is_empty()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        for item in &ctx.config.items {
            if ctx.dry_run {
                ctx.log.dry_run(&format!("would process {}", item));
                continue;
            }
            ctx.log.debug(&format!("processing {}", item));
        }
        if ctx.dry_run { return Ok(TaskResult::DryRun); }
        Ok(TaskResult::Ok)
    }
}
```

Register in `commands/install.rs`: `Box::new(tasks::my_module::MyTask)`.

## Task Helper Abstractions

For tasks that process a batch of config items (packages, symlinks, extensions, etc.), use the `ConfigBatchProcessor` helper:

```rust
use super::helpers::ConfigBatchProcessor;

pub struct MyTask;
impl Task for MyTask {
    fn name(&self) -> &str { "My task" }
    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.items.is_empty()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut processor = ConfigBatchProcessor::new();
        
        for item in &ctx.config.items {
            if is_already_in_desired_state(item) {
                ctx.log.debug(&format!("ok: {} (already ok)", item));
                processor.stats.already_ok += 1;
            } else if ctx.dry_run {
                ctx.log.dry_run(&format!("would process: {}", item));
                processor.stats.changed += 1;
            } else {
                process_item(item)?;
                ctx.log.debug(&format!("processed: {}", item));
                processor.stats.changed += 1;
            }
        }
        
        Ok(processor.finish(ctx))
    }
}
```

The `ConfigBatchProcessor` provides:
- Automatic stats tracking (`changed`, `already_ok`, `skipped`)
- Consistent summary logging via `finish(ctx)`
- Correct `TaskResult` based on dry-run mode

Additional logging helpers available in `tasks/helpers`:
- `log_already_ok(ctx, item)` — item already in desired state
- `log_would_change(ctx, item, action)` — dry-run mode change
- `log_changed(ctx, item, action)` — actual change made
- `log_skipped(ctx, item, reason)` — item skipped

## Profile Resolution

For commands that need profile resolution without full config loading, use `CommandSetup::resolve_profile()`:

```rust
let profile = CommandSetup::resolve_profile(
    cli_profile.as_deref(),
    &root,
    &platform,
    log
)?;
```

This abstracts the profile resolution logic (CLI arg → git config → interactive prompt) separately from config loading.

## Context Struct

```rust
pub struct Context<'a> {
    pub config: &'a Config, pub platform: &'a Platform, pub log: &'a Logger,
    pub dry_run: bool, pub home: PathBuf,
}
```

Helpers: `ctx.root()`, `ctx.symlinks_dir()`, `ctx.hooks_dir()`.

## Exec Helpers

`exec::run()` (fails on non-zero), `exec::run_in()` (in dir), `exec::run_in_with_env()` (in dir with env vars), `exec::run_unchecked()` (allows failure), `exec::which()` (PATH check).

## Config Loader Pattern

Each `config/*.rs` module: `ini::parse_sections(path)` → `ini::filter_sections_and()` → parse items.

## Error Handling

Use `anyhow::Result`, `?`, `.context("msg")?`, `bail!("msg")`. No `unwrap()` in non-test code.

## Rules

- All task logic in `cli/src/tasks/*.rs` — never in shell scripts
- Every task: `name`, `should_run`, `run`; check `ctx.dry_run` before side effects
- Guard tools with `exec::which()`; return `TaskResult::Skipped(reason)` when not applicable
- Add `#[cfg(test)] mod tests` to every module; use `Platform::new()` in tests
