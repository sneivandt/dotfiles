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
- **Idempotent**: Every run converges to the declared state without repeating
  unnecessary mutations.
- **Cross-Platform**: Single Rust binary; thin POSIX shell and PowerShell
  wrappers.
- **Declarative**: Configuration lives in `conf/` TOML files, deserialized via
  Serde.
- **Extensible**: A private overlay repository can inject additional config and
  custom script tasks.

## Architecture Snapshot

| Layer | Path | Role |
|---|---|---|
| Rust engine | `cli/` | Cargo project: config parsing, symlinks, file ops, orchestration |
| Shell wrappers | `dotfiles.sh` / `dotfiles.ps1` | Download or `cargo build` the binary, then exec it |
| Configuration | `conf/` | Declarative TOML config files |
| Symlinks | `symlinks/` | Managed by the Rust engine |
| Skills | `.agents/skills/` | Shared coding patterns for AI agents |
| Docs | `docs/` | Human-readable guides and reference |

The engine layers are `config/` (TOML parsing) -> `resources/` (idempotent
primitives) -> `engine/` (parallel execution) -> `tasks/`
(dependency-ordered tasks) -> `commands/` (CLI entry points). Start with
`cli/src/lib.rs` for the module map and `docs/ARCHITECTURE.md` for the full
system design.

## Conventions

### Strict Lints

The project enforces pedantic + nursery Clippy lints. `cli/Cargo.toml` is the
source of truth for denied Rust and Clippy lints. Avoid `.unwrap()` and
`.expect()` in production code; use `?` with `anyhow::Result` or return typed
errors from `cli/src/error.rs`. Test-only panics must be covered by explicit,
reasoned lint allows. Every `#[allow(...)]` must include a `reason = "..."`
argument.

### Task Patterns

Use `resource_task!` for config-backed resource tasks in `cli/src/tasks/`.
Use `task_metadata!` for hand-written tasks that need custom control flow but
still have static name, phase, domain, policy, and dependency metadata.
Dependencies use `task_deps!`. Config sections use `config_section!`. See the
`resource-implementation` and `rust-patterns` skills.

## Working with This Codebase

### Before Making Changes

- Check `.agents/skills/` for detailed technical patterns and conventions.
- Refer to `docs/CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, and
  `docs/TROUBLESHOOTING.md` for context.

### Code Quality

**Rust code** lives in `cli/`. Follow standard Rust idioms (use
`anyhow::Result`, derive `clap` args, etc.). Never leave trailing whitespace in
any file.

**Rust validation** - for Rust changes, run:

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Before committing or pushing staged Rust changes, validate the full staged set
even when some changes were authored by another session.

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

See the `testing-patterns` skill and `docs/TESTING.md` for integration test
helpers, snapshot workflows, and targeted validation guidance.

**Configuration validation** - for changes to `conf/`, `symlinks/`, `hooks/`,
wrapper scripts, or sparse-checkout manifests, run:

```bash
./dotfiles.sh test
```

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
