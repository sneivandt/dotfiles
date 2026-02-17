# GitHub Copilot Project Instructions

Core guidance for AI code assistants working on this dotfiles project.

## Project Overview

This project manages dotfiles and system configuration using a profile-based sparse checkout approach for Linux and Windows. A **Rust binary** in `cli/` is the engine; the shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) only bootstrap and invoke it.

**Core Principles**:
- **Profile-Based**: Profiles (base, arch, arch-desktop, desktop, windows) control sparse checkout and configuration
- **Idempotent**: Every run converges to the declared state without side effects
- **Cross-Platform**: Single Rust binary; thin POSIX shell and PowerShell wrappers
- **Declarative**: Configuration lives in `conf/` INI files, parsed natively by the Rust engine

## Architecture

| Layer | Path | Role |
|---|---|---|
| Rust engine | `cli/` | Cargo project — config parsing, symlinks, file ops, orchestration |
| Shell wrappers | `dotfiles.sh` / `dotfiles.ps1` | Download or `cargo build` the binary, then exec it |
| Configuration | `conf/` | INI files (unchanged from previous design) |
| Symlinks | `symlinks/` | Managed by the Rust engine |
| Skills | `.github/skills/` | Agent-specific coding patterns |
| Docs | `docs/` | Human-readable guides and reference |

The Rust binary uses **clap** for CLI parsing and **anyhow** for error handling. It handles all file operations natively and only shells out for package managers (`pacman`, `paru`, `winget`) and service management (`systemctl`).

See `docs/ARCHITECTURE.md` for the full system design.

## Working with This Codebase

### Before Making Changes

- Check `.github/skills/` for detailed technical patterns and conventions
- Refer to `docs/CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/CUSTOMIZATION.md`, and `docs/TROUBLESHOOTING.md` for context

### Code Quality

**Rust code** lives in `cli/`. Follow standard Rust idioms (use `anyhow::Result`, derive `clap` args, etc.). Never leave trailing whitespace in any file.

**Testing** — always run before committing:
```bash
cd cli && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

**Integration tests** run alongside unit tests via `cargo test` and exercise config parsing, symlinking, and dry-run behaviour.

**Dry-run** — preview changes without applying:
```bash
./dotfiles.sh install -d
```

### Security

- Run `code_review` and `codeql_checker` tools before finalising
- Never commit secrets, credentials, or sensitive data
- Fix any security issues found

## Documentation Structure

- **`.github/copilot-instructions.md`** — this file (universal agent guidance)
- **`.github/skills/`** — agent-specific technical patterns and conventions
- **`docs/`** — human-readable guides (also useful for agents needing context)

When in doubt, check skills for technical patterns and docs for procedures.
