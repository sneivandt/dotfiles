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
├── resources/     # Declarative resource abstraction
│   ├── mod.rs     # Resource trait, ResourceState, ResourceChange
│   └── *.rs       # Per-type resources (symlink, registry, chmod, etc.)
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
        // Prefer capability-based checks over direct OS checks for expressiveness
        ctx.platform.supports_systemd() && !ctx.config.items.is_empty()
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

## Platform Detection

Use capability-based methods when possible for more expressive code:

**Capability Methods** (preferred):
- `platform.supports_chmod()` — POSIX file permissions support
- `platform.supports_systemd()` — systemd support
- `platform.has_registry()` — Windows Registry support
- `platform.supports_aur()` — AUR package support
- `platform.uses_pacman()` — pacman package manager

**Direct OS Checks** (when capabilities don't apply):
- `platform.is_linux()` — Linux OS
- `platform.is_windows()` — Windows OS
- `platform.is_arch_linux()` — Arch Linux specifically

**Why use capability methods?** They make the *reason* for the platform check explicit:

```rust
// Less expressive - why does Linux matter?
ctx.platform.is_linux() && !ctx.config.units.is_empty()

// More expressive - clearly about systemd support
ctx.platform.supports_systemd() && !ctx.config.units.is_empty()
```

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

## Resource Abstraction

The `resources/` module provides a declarative layer for checking and applying system state:

```rust
pub trait Resource {
    fn description(&self) -> String;
    fn current_state(&self) -> Result<ResourceState>;
    fn needs_change(&self) -> Result<bool>;
    fn apply(&self) -> Result<ResourceChange>;
}
pub enum ResourceState { Missing, Correct, Incorrect { current: String }, Invalid { reason: String } }
pub enum ResourceChange { Applied, AlreadyCorrect, Skipped { reason: String } }
```

Tasks use `Resource` implementors (e.g., `SymlinkResource`, `RegistryResource`) to check state and apply changes rather than doing it inline. New declarative resources go in `resources/*.rs`.

## Rules

- All task logic in `cli/src/tasks/*.rs` — never in shell scripts
- Every task: `name`, `should_run`, `run`; check `ctx.dry_run` before side effects
- Use `Resource` trait for declarative state checks where applicable
- Guard tools with `exec::which()`; return `TaskResult::Skipped(reason)` when not applicable
- Add `#[cfg(test)] mod tests` to every module; use `Platform::new()` in tests
