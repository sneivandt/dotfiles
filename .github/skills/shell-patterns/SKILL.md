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

Shell scripts in this project are **thin wrappers** (~150 lines) that bootstrap the Rust binary. All task logic lives in `cli/src/tasks/bootstrap/`, `cli/src/tasks/repository/`, and `cli/src/tasks/apply/`.

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

1. **Production mode** (default): Downloads latest binary from GitHub Releases if missing, verifies checksum, then lets the binary self-update
2. **Build mode** (`--build`): Builds from source with `cargo build --release`, runs directly

```sh
if [ "$BUILD_MODE" = true ]; then
  cd "$DOTFILES_ROOT/cli"
  cargo build --release --quiet
  exec "$DOTFILES_ROOT/cli/target/release/dotfiles" --root "$DOTFILES_ROOT" $ARGS
fi
```

The wrapper resolves `DOTFILES_ROOT`, handles bootstrap/build concerns, and
otherwise forwards arguments to the Rust binary unchanged. The Rust CLI owns
argument validation.

### Binary Auto-Update

After bootstrap, the Rust binary handles version caching, update checks,
checksum verification, and re-exec.

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

- Keep wrapper scripts as short as practical (dotfiles.sh ~180 lines, dotfiles.ps1 ~300 lines)
- Never add task logic to shell scripts — use `cli/src/tasks/bootstrap/`, `cli/src/tasks/repository/`, or `cli/src/tasks/apply/`
- The wrapper must resolve and export `DOTFILES_ROOT` before launching the binary
- The wrapper must export `DOTFILES_WRAPPER` (`sh` or `pwsh`) so the CLI knows which wrapper invoked it
- Wrapper arguments should pass through to the Rust CLI unless the wrapper itself must consume them (for example `--build`)
