---
name: shell-patterns
description: >
  Use for dotfiles.sh, dotfiles.ps1, or POSIX hook scripts: bootstrap,
  checksum/provenance verification, wrapper-owned flags, argument forwarding,
  and shell portability. Not for application tasks.
---

# Shell Patterns

Wrappers bootstrap and invoke the Rust binary. Domain behavior belongs under
`cli/src/`.

## Wrapper boundary

Wrappers may own:

- production download/bootstrap
- checksum and provenance verification
- source build mode
- flags consumed before the binary exists
- unchanged forwarding of all other arguments

The Rust binary owns normal CLI validation, task behavior, and self-update after
bootstrap.

When changing a wrapper, compare its shell/PowerShell counterpart and existing
wrapper fixtures. Cover consumed `--build`, empty arguments, spaces, arguments
after `--`, stdout/stderr routing, and the child's exit code. Do not reconstruct
forwarded arguments through `eval` or an interpolated command string.

Downloads must be verified before execution or replacement of the cached binary.
Preserve temporary-file cleanup, rejection of mismatched checksums/provenance,
and the working binary on failure. Never weaken verification to work around a
network or test failure.

## Portability

- POSIX scripts use `#!/bin/sh`; do not add Bash-only syntax.
- Enable error and unset-variable handling.
- Quote expansions and keep conditionals portable.
- PowerShell uses terminating errors and path helpers.
- Explicitly handle native process exit codes in PowerShell. In POSIX shell,
  do not assume `set -e` catches failures in every conditional or pipeline.
- Hook scripts remain small orchestrators over focused `hooks/check-*.sh`
  helpers.

## Output

Normal wrapper progress follows the CLI's compact ` · `-separated style. Keep
errors and warnings on stderr with their existing prefixes. Avoid free-form
ellipsis progress and duplicate status lines.

Use `git-hooks-patterns` for staged checks, `logging-patterns` for the Rust
console contract, and [Testing](../../../docs/TESTING.md#choosing-coverage) for
lint and wrapper checks.
