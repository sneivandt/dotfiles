---
name: git-hooks-patterns
description: >
  Git hook installation and pre-commit behavior for this dotfiles repo. Use
  when changing hooks/, hook tasks, staged checks, or hook-based sensitive-data
  scanning.
---

# Git Hooks Patterns

The project uses git hooks for pre-commit security scanning. Hook installation
is handled by `cli/src/domains/git/hooks.rs`, which implements `Task`.

Hooks live in `hooks/` and are copied to `.git/hooks/` by the Rust engine.

## Hook Installation Task

`InstallGitHooks` lives in `cli/src/domains/git/hooks.rs`. Keep filesystem access
behind its injectable `FileSystemOps`; applicability and discovery must not call
`Path::exists()` or read the real filesystem directly. The app catalog owns its
cross-domain dependency on `UpdateRepository`.

`discover_hooks()` takes `ctx` and a `&Arc<dyn FileSystemOps>` argument and reads the
`hooks/` directory via `fs_ops.read_dir()`, returning one `HookFileResource` per file
that has no extension (conventional hook scripts such as `pre-commit`, `commit-msg`).

## Sensitive Data Detection

### Pattern Configuration

Patterns in `hooks/sensitive-patterns.ini` are grouped by INI section and use
extended regular expressions. Match contextual indicators around a secret,
not secret-like character sequences alone, to limit false positives.

### Pre-commit Hook

The `hooks/pre-commit` script is a POSIX shell (`#!/bin/sh`) orchestrator that
delegates to dedicated scripts:
1. Runs `hooks/check-sensitive.sh` which reads patterns from `hooks/sensitive-patterns.ini` and scans `git diff --cached` for matches
2. Runs `hooks/check-rust.sh` for staged Rust/script checks (for the canonical full local Rust/cross-platform sequence, see `cross-platform-verification`)
3. In full mode, runs `hooks/check-ci-guards.sh` which mirrors targeted CI checks for staged config, dependency, and shell-wrapper changes:
   - `conf/*.toml` or `symlinks/` changes: shell config validation
   - `cli/Cargo.toml`, `cli/Cargo.lock`, or `cli/deny.toml` changes: wildcard dependency scan
   - shell hook/wrapper changes: ShellCheck on staged shell files when installed
   - Windows-target clippy, Rust tests, config drift tests, cargo-deny, and Linux shell wrapper tests
4. Prints error and aborts if any check fails

Use `git commit --no-verify` only for a confirmed false positive.

## Adding New Patterns

1. Write ERE pattern and add to appropriate section in `hooks/sensitive-patterns.ini`
2. Test: `echo "apikey=test" | grep -iE "pattern"`
3. Be specific to reduce false positives — match context around the secret

- Hooks are installed as copies, so rerun install after changing them.
- Keep `hooks/pre-commit` and
  `.github/workflows/scripts/linux/test-git-hooks.sh` synchronized when adding a
  `hooks/check-*.sh` helper.
