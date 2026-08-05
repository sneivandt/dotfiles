---
name: shell-patterns
description: >
  Thin wrapper and POSIX hook conventions for this dotfiles repo. Use when
  modifying dotfiles.sh, dotfiles.ps1, wrapper bootstrap/argument forwarding,
  or POSIX hook scripts.
---

# Shell Wrapper Patterns

Shell scripts in this project are **thin wrappers** that bootstrap the Rust
binary. Task entry points live directly under `cli/src/domains/<domain>/`.

## Entry Point: `dotfiles.sh`

The main shell script resolves and exports `DOTFILES_ROOT`, identifies itself
through `DOTFILES_WRAPPER`, handles bootstrap, and forwards arguments to the
Rust engine.

### Two Modes

1. **Production mode** (default): Downloads latest binary from GitHub Releases if missing, verifies checksum and build provenance, then lets the binary self-update
2. **Build mode** (`--build`): Builds from source with `cargo build --profile dev-opt`, runs directly

The wrapper resolves `DOTFILES_ROOT`, handles bootstrap/build concerns, and
otherwise forwards arguments to the Rust binary unchanged. The Rust CLI owns
argument validation.

### Binary Auto-Update

After bootstrap, the Rust binary handles version caching, update checks,
checksum and build provenance verification, and re-exec.

## Entry Point: `dotfiles.ps1`

Windows PowerShell wrapper with identical logic:
- `--build` flag for build-from-source mode
- Downloads `dotfiles-windows-x86_64.exe` from releases
- Same bootstrap download, checksum, and build provenance verification

## Git Hooks

The `hooks/pre-commit` script is the only other POSIX shell entrypoint. It scans
staged changes for sensitive patterns and runs staged Rust/PowerShell checks.
With `DOTFILES_HOOKS_FULL=1`, it also delegates targeted CI parity checks to
`hooks/check-ci-guards.sh`.

When editing `dotfiles.sh`, keep argument forwarding unchanged unless the wrapper
itself consumes a flag. In full hook mode, `hooks/check-ci-guards.sh` runs
ShellCheck on staged shell files and the Linux wrapper test script for staged
`dotfiles.sh` changes.

## Code Style Rules

- Always `#!/bin/sh` with `set -o errexit` and `set -o nounset`
- Use compact conditionals: `if [ condition ]; then`
- Quote all variable expansions
- No Bash features (arrays, process substitution)
- Keep wrapper scripts minimal — all logic belongs in Rust

## Console Output Style

Wrapper output is the first thing a user sees, before the Rust binary exists, so
it must match the CLI's console style rather than inventing its own.

The CLI writes a single ` · `-separated header, then status lines, then a
` · `-separated summary:

```text
Install · profile desktop · Arch Linux · dry run · overlay ~/src/dotfiles-private
⊘ Dotfiles repository · local changes present
No changes · 15 up to date · 1 ignored · 0.7s
```

The `dry run` and `overlay <path>` sections are optional and always come last,
in that order. The CLI renders the header dim; wrappers have no equivalent
styling and print theirs plain.

Bootstrap download output follows the same shape — a header naming the resolved
tag and target, and a completion line stating what was verified:

```text
Bootstrap · dotfiles v2026.07.25-1 · linux-x86_64
Downloaded · checksum verified · /home/user/src/dotfiles/bin/dotfiles
```

Do not use trailing-ellipsis progress prose. Errors and warnings keep the
`ERROR: ` / `WARNING: ` prefixes on stderr (`Write-Error` / `Write-Warning` in
PowerShell); the ` · ` style is for normal progress only.

## When to Edit Shell Scripts

Edit `dotfiles.sh` or `dotfiles.ps1` only for:
- Binary download/update logic changes
- New CLI flags that need wrapper-level handling
- Bootstrap behavior before the Rust binary is available

For everything else (tasks, config, logging), edit the Rust code in `cli/src/`.

## Rules

- Keep wrapper scripts as short as practical; avoid line-count targets that
  encourage moving domain behavior into wrappers
- Never add task logic to shell scripts — use a root task entry module under
  `cli/src/domains/<domain>/`
