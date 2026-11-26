---
name: git-hooks-patterns
description: >
  Use for hooks/, InstallGitHooks, staged check orchestration, or sensitive
  pattern/allowlist behavior. Not for general CI workflow changes or
  dotfiles.sh/.ps1 bootstrap wrappers (use self-update).
---

# Git Hooks Patterns

## Installation and ownership

- `InstallGitHooks` installs extensionless files from `hooks/` via Git's common
  dir or `core.hooksPath` (a linked worktree has a `.git` file, not a hooks
  directory). Keep directory enumeration in `run()`, not `should_run()`.
- By default the installed `pre-commit` entry point is a copy, while it resolves
  `hooks/check-*.sh` helpers from the checkout root at runtime (`pre-commit`
  runs `check-sensitive.sh`, `check-rust.sh`, and fast checks in
  `check-ci-guards.sh`; `DOTFILES_HOOKS_FULL=1` enables the heavier checks in
  `check-rust.sh` and `check-ci-guards.sh`). Entry-point edits need
  redeployment; helper-only edits take effect immediately.
- If `core.hooksPath` already points at the tracked `hooks/` sources, preserve
  them rather than copying over or removing them.

## Staged-content and sensitive-pattern rules

- All scripts under `hooks/` are POSIX `sh` (`#!/bin/sh`).
- Scan staged Git blobs, not working-tree files—a partially staged file's
  working-tree content may differ from what is being committed. Cover spaces in
  filenames, renames, deletions, and empty diffs.
- `hooks/sensitive-patterns.ini` and `hooks/sensitive-allowlist.ini` use
  extended regexes. Allowlist rules strip only the matched safe span so the
  remainder of the line is still scanned; test both that the false positive
  clears and that a real secret on the same line still trips detection.

## Verification

See [Wrapper and hook tests](../../../docs/content/development/testing.md#wrapper-and-hook-tests).
`test-git-hooks.sh` runs commits in its own disposable temp repository without
touching the caller's checkout. Use `ci-cd-patterns` when changing CI parity scope.
