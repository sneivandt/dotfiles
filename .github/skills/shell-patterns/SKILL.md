---
name: shell-patterns
description: >
  Patterns for the thin shell wrapper scripts (dotfiles.sh and hooks).
  Use when modifying the entry point scripts or POSIX shell hooks.
metadata:
  author: sneivandt
  version: "2.0"
---

# Shell Wrapper Patterns

Shell scripts in this project are **thin wrappers** (~150 lines) that bootstrap the Rust binary. All task logic lives in `cli/src/tasks/*.rs`.

## Entry Point: `dotfiles.sh`

The main shell script handles binary management and forwards args to the Rust engine:

```sh
#!/bin/sh
set -o errexit
set -o nounset

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT
```

### Two Modes

1. **Production mode** (default): Downloads latest binary from GitHub Releases, verifies checksum, caches version
2. **Build mode** (`--build`): Builds from source with `cargo build --release`, runs directly

```sh
if [ "$BUILD_MODE" = true ]; then
  cd "$DOTFILES_ROOT/cli"
  cargo build --release --quiet
  exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" $ARGS
fi
```

All arguments except `--build` are forwarded to the Rust binary unchanged.

### Binary Auto-Update

The wrapper checks for updates with a 1-hour cache:
- Reads cached version from `bin/.dotfiles-version-cache`
- Compares with GitHub Releases API
- Downloads and verifies SHA256 checksum if outdated
- Falls back to existing binary if GitHub is unreachable

## Entry Point: `dotfiles.ps1`

Windows PowerShell wrapper with identical logic:
- `-Build` switch for build-from-source mode
- Downloads `dotfiles-windows-x86_64.exe` from releases
- Same caching and checksum verification

## Git Hooks

The `hooks/pre-commit` script is the only other POSIX shell script. It scans staged changes for sensitive patterns.

## Code Style Rules

- Always `#!/bin/sh` with `set -o errexit` and `set -o nounset`
- Use compact conditionals: `if [ condition ]; then`
- Quote all variable expansions
- No Bash features (arrays, process substitution)
- Keep wrapper scripts minimal — all logic belongs in Rust

## When to Edit Shell Scripts

Edit `dotfiles.sh` or `dotfiles.ps1` only for:
- Binary download/update logic changes
- New CLI flags that need wrapper-level handling
- Version caching behavior

For everything else (tasks, config, logging), edit the Rust code in `cli/src/`.

## Rules

- Wrapper scripts must stay under ~150 lines
- Never add task logic to shell scripts — use `cli/src/tasks/*.rs`
- The `--root` flag is always passed to the binary by the wrapper
- Wrapper forwards all other arguments unchanged via `exec`
