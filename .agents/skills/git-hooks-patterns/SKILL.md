---
name: git-hooks-patterns
description: >
  Use for hooks/, InstallGitHooks, staged check orchestration, or sensitive
  pattern/allowlist behavior. Not for general CI workflow changes.
---

# Git Hooks Patterns

## Installation

`InstallGitHooks` discovers extensionless files under `hooks/` through injected
`FileSystemOps` and copies them into `.git/hooks/`. Keep filesystem access out
of applicability checks. Cross-domain dependency wiring belongs in the app
catalog.

## Pre-commit contract

`hooks/pre-commit` is a POSIX `sh` orchestrator:

- `check-sensitive.sh` scans staged diffs.
- `check-rust.sh` runs staged Rust/script checks.
- `check-ci-guards.sh` adds targeted CI parity when
  `DOTFILES_HOOKS_FULL=1`.

Keep the hook and its integration test synchronized when adding a helper.
Installed hooks are copies, so rerun install after editing.

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

The full hook integration script creates commits and requires a clean scratch
repository; do not run it against a dirty checkout.

Use `shell-patterns` for POSIX conventions and `ci-cd-patterns` when changing CI
parity scope.
