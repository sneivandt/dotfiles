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
