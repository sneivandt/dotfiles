---
name: git-hooks-patterns
description: >
  Use for hooks/, InstallGitHooks, staged check orchestration, or sensitive
  pattern/allowlist behavior. Not for general CI workflow changes.
---

# Git Hooks Patterns

## Installation

`InstallGitHooks` discovers extensionless files under `hooks/` through injected
`FileSystemOps` and copies them into `.git/hooks/`. Keep directory enumeration in
execution; applicability uses only cheap injected existence checks.
Cross-domain dependency wiring belongs in the app catalog.

## Pre-commit contract

`hooks/pre-commit` is a POSIX `sh` orchestrator:

- `check-sensitive.sh` scans staged diffs.
- `check-rust.sh` runs staged Rust/script checks.
- `check-ci-guards.sh` adds targeted CI parity when
  `DOTFILES_HOOKS_FULL=1`.

Keep the hook and its integration test synchronized when adding a helper.
The installed entry point is a copy, but it resolves `.sh` helpers from the
checkout at runtime. A copied entry-point change needs explicit redeployment;
helper-only changes do not. Do not run the whole installer to test a hook.

Preserve staged-content semantics: a partially staged file's working-tree
contents are not necessarily what will be committed. Cover spaces in filenames,
renames, deletions, and empty diffs when changing discovery or filtering.

## Sensitive patterns

- Detection rules live in `hooks/sensitive-patterns.ini`; safe-span exceptions
  live in `hooks/sensitive-allowlist.ini`.
- Patterns and allowlists are extended regular expressions.
- Match contextual indicators, not generic secret-like strings.
- Allowlist only the safe span; the remainder of the line must still be scanned.
- Prefer a narrow allowlist entry to weakening detection or recommending
  `--no-verify`.
- Test both directions: the false positive clears and a real secret on the same
  line remains detected.

The full hook integration script creates commits. Run it only in a fresh
disposable repository, never the user's checkout even when clean. Follow the
scratch-repository procedure in
[Wrapper and hook tests](../../../docs/TESTING.md#wrapper-and-hook-tests).

Use `shell-patterns` for POSIX conventions and `ci-cd-patterns` when changing CI
parity scope.
