# Dotfiles Agent Instructions

## Project Model

This repository manages Linux and Windows dotfiles through a profile-based
sparse checkout system. The Rust binary in `cli/` is the engine. The shell
wrappers (`dotfiles.sh`, `dotfiles.ps1`) only bootstrap and invoke it.

Core principles:

- **Profile-based**: `base`, `desktop`, and platform categories decide what is
  checked out and applied.
- **Idempotent**: every run converges to declared state without repeating
  unnecessary mutations.
- **Cross-platform**: one Rust engine with thin POSIX shell and PowerShell
  wrappers.
- **Declarative**: configuration lives in `conf/` TOML files and is parsed with
  Serde.
- **Extensible**: a private overlay can inject config and custom script tasks.

## Architecture Map

| Layer | Path | Role |
|---|---|---|
| Rust engine | `cli/` | Config parsing, resource primitives, task orchestration, CLI commands |
| Configuration | `conf/` | Declarative TOML inputs |
| Symlinks | `symlinks/` | Files managed by the engine |
| Shell wrappers | `dotfiles.sh`, `dotfiles.ps1` | Bootstrap or build the Rust binary, then exec it |
| Docs | `docs/` | Human-facing guides and reference |

CLI execution flow:

```text
cli/main -> commands -> config/context -> tasks -> engine scheduler/resource processing -> resources
```

Start with `cli/src/lib.rs` for the module map and `docs/ARCHITECTURE.md` for
the full design.

## Before Editing

1. Prefer existing patterns, macros, validators, and helpers over new ad-hoc
   logic.
2. Keep changes narrow, idempotent, and cross-platform unless the task is
   explicitly platform-specific.
3. Do not commit secrets, tokens, private URLs, or employer-confidential
   content.

## Rust Conventions

- `cli/Cargo.toml` defines strict Rust, Clippy pedantic, and Clippy nursery
  denies.
- Avoid `.unwrap()` and `.expect()` in production code. Use `?` with
  `anyhow::Result` or typed errors from `cli/src/error.rs`.
- Test-only panics need explicit, reasoned lint allows.
- Every `#[allow(...)]` must include `reason = "..."`.
- Use `resource_task!` for config-backed resource tasks in `cli/src/tasks/`.
- Use `task_metadata!` and `task_deps!` for custom task control flow.
- Use `config_section!` for TOML-backed config sections.
- Keep platform-specific imports and logic behind the appropriate `#[cfg(...)]`
  gates.

## Validation

Run only the checks relevant to the files changed. This table is a quick
starting point; see `docs/TESTING.md` and `docs/CONTRIBUTING.md` for the
complete validation workflow and targeted checks.

| Changed files | Validation |
|---|---|
| Rust in `cli/` | `cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` |
| Rust in `cli/` with platform-sensitive paths, imports, symlinks, registry, or cfg gates | Also run `cd cli && cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings` |
| `conf/`, `symlinks/`, `hooks/`, wrappers, or sparse-checkout manifests | `./dotfiles.sh test` |
| AI tooling, agent, or local plugin configuration | `./dotfiles.sh install -d` |

If the Windows Rust target is unavailable and not appropriate to install, say so
explicitly. Before committing or pushing staged Rust changes, validate the full
staged set even if another session authored part of it.