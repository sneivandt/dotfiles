# Dotfiles Agent Instructions

Core guidance for AI coding agents working on this dotfiles project. This file
is the shared source of truth for Copilot CLI, Codex CLI, and other agents that
read `AGENTS.md`.

## Project Overview

This project manages dotfiles and system configuration using a profile-based
sparse checkout approach for Linux and Windows. A **Rust binary** in `cli/` is
the engine; the shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) only bootstrap
and invoke it.

**Core Principles**:
- **Profile-Based**: Profiles (base, desktop) control sparse checkout and
  configuration; platform categories (linux, windows, arch) are auto-detected.
- **Idempotent**: Every run converges to the declared state without side
  effects.
- **Cross-Platform**: Single Rust binary; thin POSIX shell and PowerShell
  wrappers.
- **Declarative**: Configuration lives in `conf/` TOML files, deserialized via
  Serde.
- **Extensible**: A private overlay repository can inject additional config and
  custom script tasks.

## Architecture

| Layer | Path | Role |
|---|---|---|
| Rust engine | `cli/` | Cargo project: config parsing, symlinks, file ops, orchestration |
| Shell wrappers | `dotfiles.sh` / `dotfiles.ps1` | Download or `cargo build` the binary, then exec it |
| Configuration | `conf/` | Declarative TOML config files |
| Symlinks | `symlinks/` | Managed by the Rust engine |
| Skills | `.agents/skills/` | Shared agent-specific coding patterns for Copilot and Codex |
| Docs | `docs/` | Human-readable guides and reference |

The engine has five internal layers: `config/` (TOML parsing) -> `resources/`
(idempotent primitives) -> `engine/` (parallel execution) -> `tasks/`
(dependency-ordered tasks) -> `commands/` (CLI entry points). See
`docs/ARCHITECTURE.md` for the full system design.

## Key Files

| File | Purpose |
|---|---|
| `cli/src/lib.rs` | Module structure and public API docs; start here |
| `cli/src/cli.rs` | clap-based CLI args and `GlobalOpts` |
| `cli/src/tasks/mod.rs` | `Task` trait definition |
| `cli/src/tasks/macros.rs` | `resource_task!` and `task_deps!` macros |
| `cli/src/tasks/catalog.rs` | Task registry (`all_install_tasks()` / `all_uninstall_tasks()`) |
| `cli/src/resources/mod.rs` | `Resource`, `IntrinsicState`, and `ResourceStateProvider` primitives |
| `cli/src/engine/orchestrate.rs` | Provider-backed resource orchestration workhorse |
| `cli/src/engine/plan.rs` | Pure resource plan/diff construction before mutation |
| `cli/src/config/mod.rs` | Config loading and `config_section!` re-export |
| `cli/src/error.rs` | `ResourceError` and `ConfigError` domain types |

## Conventions

### Strict Lints

The project enforces pedantic + nursery Clippy lints. `cli/Cargo.toml` is the
source of truth for denied Rust and Clippy lints. Never use `.unwrap()` or
`.expect()`; use `?` with `anyhow::Result` or return typed errors from
`cli/src/error.rs`. Every `#[allow(...)]` must include a `reason = "..."`
argument.

### Macro-Driven Tasks

Tasks are defined via the `resource_task!` macro in `cli/src/tasks/`, not by
hand-implementing the `Task` trait. Dependencies use `task_deps!`. Config
sections use `config_section!`. See the `resource-implementation` and
`rust-patterns` skills.

### Resource State Providers

- `Resource`: core operations (describe, apply, remove).
- `IntrinsicState`: resources that can check their own state with
  `current_state()`.
- `ResourceStateProvider`: supplies state for orchestration, either via
  intrinsic checks or cached/bulk queries.

The provider split lets intrinsic checks and bulk/cached state queries share the
same orchestration path.

### Category Filtering

Every config item is category-aware. Platform categories (linux, windows, arch)
are auto-detected; profile categories (base, desktop) are user-selected. Items
matching is AND logic within a category group. See
`cli/src/config/helpers/category_matcher.rs`.

### Re-exec on Self-Update

After the binary updates itself, it re-execs with a guard env var
(`DOTFILES_REEXEC_GUARD`) to prevent infinite loops.

## Working with This Codebase

### Before Making Changes

- Check `.agents/skills/` for detailed technical patterns and conventions.
- Refer to `docs/CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, and
  `docs/TROUBLESHOOTING.md` for context.

### Code Quality

**Rust code** lives in `cli/`. Follow standard Rust idioms (use
`anyhow::Result`, derive `clap` args, etc.). Never leave trailing whitespace in
any file.

**Testing** - always run before committing:

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

**Cross-platform verification** - Windows-only CI failures are a recurring
issue. After any Rust change, also run:

```bash
cd cli && cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
```

This catches missing `#[cfg(windows)]` arms, broken `winreg` references, and
platform-gated import drift before they hit CI. The pre-commit hook runs this
automatically when the toolchain is installed (`rustup target add
x86_64-pc-windows-gnu` + a mingw-w64 gcc). When the target is unavailable and
not appropriate to install, say so explicitly in the response. See the
`cross-platform-verification` skill for details.

Integration tests use `IntegrationTestContext` from `cli/tests/common/` and
exercise config parsing, symlinking, and dry-run behaviour. Snapshot tests use
`insta`; update with `INSTA_UPDATE=unseen cargo test`. See `docs/TESTING.md`
for details.

**Dry-run** - preview changes without applying:

```bash
./dotfiles.sh install -d
```

### Security

- Never commit secrets, credentials, or sensitive data.
- Review git hooks patterns in the `git-hooks-patterns` skill.
- Fix any security issues found.

## Documentation Structure

- **`AGENTS.md`** - shared repo-wide agent guidance for Copilot, Codex, and
  other agents.
- **`.agents/skills/`** - shared technical patterns and task-specific agent
  guidance.
- **`docs/`** - human-readable guides and reference.

Key references: `docs/ARCHITECTURE.md` (system design),
`docs/CONTRIBUTING.md` (development workflow), `docs/TESTING.md` (test
strategy), `docs/PROFILES.md` (profile system), `docs/CONFIGURATION.md` (TOML
format).

**Where to put new content:**
- Universal agent rules or project overview -> `AGENTS.md`.
- Technical coding patterns or format specs -> a skill in `.agents/skills/`.
- User guides, procedures, or troubleshooting -> a doc in `docs/`.

When in doubt, check skills for technical patterns and docs for procedures.
