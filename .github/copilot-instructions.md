# GitHub Copilot Project Instructions

Core guidance for AI code assistants working on this dotfiles project.

## Project Overview

This project manages dotfiles and system configuration using a profile-based sparse checkout approach for Linux and Windows. A **Rust binary** in `cli/` is the engine; the shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) only bootstrap and invoke it.

**Core Principles**:
- **Profile-Based**: Profiles (base, desktop) control sparse checkout and configuration; platform categories (linux, windows, arch) are auto-detected
- **Idempotent**: Every run converges to the declared state without side effects
- **Cross-Platform**: Single Rust binary; thin POSIX shell and PowerShell wrappers
- **Declarative**: Configuration lives in `conf/` TOML files, deserialized via Serde
- **Extensible**: A private overlay repository can inject additional config and custom script tasks

## Architecture

| Layer | Path | Role |
|---|---|---|
| Rust engine | `cli/` | Cargo project — config parsing, symlinks, file ops, orchestration |
| Shell wrappers | `dotfiles.sh` / `dotfiles.ps1` | Download or `cargo build` the binary, then exec it |
| Configuration | `conf/` | Declarative TOML config files |
| Symlinks | `symlinks/` | Managed by the Rust engine |
| Skills | `.github/skills/` | Agent-specific coding patterns |
| Docs | `docs/` | Human-readable guides and reference |

The engine has five internal layers: `config/` (TOML parsing) → `resources/` (idempotent primitives) → `engine/` (parallel execution) → `phases/` (dependency-ordered tasks) → `commands/` (CLI entry points). See `docs/ARCHITECTURE.md` for the full system design.

## Key Files

| File | Purpose |
|---|---|
| `cli/src/lib.rs` | Module structure and public API docs — start here |
| `cli/src/cli.rs` | clap-based CLI args and `GlobalOpts` |
| `cli/src/phases/mod.rs` | `Task` trait definition and macros (`resource_task!`, `task_deps!`) |
| `cli/src/phases/catalog.rs` | Task registry (`all_install_tasks()` / `all_uninstall_tasks()`) |
| `cli/src/resources/mod.rs` | `Applicable` and `Resource` traits — the idempotent primitives |
| `cli/src/engine/orchestrate.rs` | `process_resources()` — the core execution workhorse |
| `cli/src/config/mod.rs` | `config_section!` macro and config loading |
| `cli/src/error.rs` | `ResourceError` and `ConfigError` domain types |

## Conventions

### Strict Lints

The project enforces pedantic + nursery Clippy lints and explicitly denies `panic`, `unwrap_used`, `expect_used`, `todo`, and `dbg_macro`. Never use `.unwrap()` or `.expect()` — use `?` with `anyhow::Result` or return typed errors from `cli/src/error.rs`.

### Macro-Driven Tasks

Tasks are defined via the `resource_task!` macro in `cli/src/phases/`, not by hand-implementing the `Task` trait. Dependencies use `task_deps!`. Config sections use `config_section!`. See the `resource-implementation` and `rust-patterns` skills.

### Two Resource Traits

- `Applicable`: core operations (describe, apply, remove)
- `Resource`: extends `Applicable` with `current_state()` for state checking

The split exists because some resources need bulk state queries before individual apply.

### Category Filtering

Every config item is category-aware. Platform categories (linux, windows, arch) are auto-detected; profile categories (base, desktop) are user-selected. Items matching is AND logic within a category group. See `cli/src/config/helpers/category_matcher.rs`.

### Re-exec on Self-Update

After the binary updates itself, it re-execs with a guard env var (`DOTFILES_REEXEC_GUARD`) to prevent infinite loops.

## Working with This Codebase

### Before Making Changes

- Check `.github/skills/` for detailed technical patterns and conventions
- Refer to `docs/CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, and `docs/TROUBLESHOOTING.md` for context

### Code Quality

**Rust code** lives in `cli/`. Follow standard Rust idioms (use `anyhow::Result`, derive `clap` args, etc.). Never leave trailing whitespace in any file.

**Testing** — always run before committing:
```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Integration tests use `IntegrationTestContext` from `cli/tests/common/` and exercise config parsing, symlinking, and dry-run behaviour. Snapshot tests use `insta` — update with `INSTA_UPDATE=unseen cargo test`. See `docs/TESTING.md` for details.

**Dry-run** — preview changes without applying:
```bash
./dotfiles.sh install -d
```

### Security

- Never commit secrets, credentials, or sensitive data
- Review git hooks patterns in `.github/skills/git-hooks-patterns/`
- Fix any security issues found

## Documentation Structure

- **`.github/copilot-instructions.md`** — this file (universal agent guidance)
- **`.github/skills/`** — agent-specific technical patterns and conventions
- **`docs/`** — human-readable guides (also useful for agents needing context)

Key references: `docs/ARCHITECTURE.md` (system design), `docs/CONTRIBUTING.md` (development workflow), `docs/TESTING.md` (test strategy), `docs/PROFILES.md` (profile system), `docs/CONFIGURATION.md` (TOML format).

**Where to put new content:**
- Universal agent rules or project overview → this file
- Technical coding patterns or format specs → a skill in `.github/skills/`
- User guides, procedures, or troubleshooting → a doc in `docs/`

When in doubt, check skills for technical patterns and docs for procedures.

### Creating New Skills

Skills live in `.github/skills/<skill-name>/SKILL.md`. Every `SKILL.md` starts with YAML frontmatter:

```yaml
---
name: skill-name          # kebab-case, must match directory name
description: >
  Brief description of what it covers and when to use it.
metadata:
  author: sneivandt
  version: "1.0"
---
```

Create a skill when the topic is complex, repeated, or has common pitfalls. Structure:
overview, core content with headings, code examples from the codebase, rules, and cross-references.
Aim for under 100 lines (longer is acceptable for complex topics). Write in terms of current state —
never describe something as "new" or "changed".
