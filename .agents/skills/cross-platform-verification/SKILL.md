---
name: cross-platform-verification
description: >
  How to verify Rust and shell-wrapper changes work on both Linux and Windows
  without waiting for CI. Use after any change to cli/src/, dotfiles.sh, or
  dotfiles.ps1 — especially when adding cfg gates, touching paths, symlinks,
  registry, or platform-specific imports.
---

# Cross-Platform Verification

This is the canonical source for general local Rust/cross-platform validation
commands. Other skills should reference this one instead of copying the sequence.

## Canonical local sequence (Rust changes)

```sh
cd cli
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test
```

If the Windows target/toolchain is unavailable and installing it is not
appropriate, report that explicitly.

## Shell wrapper checks

| Change touches | Run |
|---|---|
| `dotfiles.sh`, `*.sh` | `shellcheck --severity=warning --shell=sh dotfiles.sh ...` |
| `dotfiles.ps1`, `*.ps1`, `*.psm1` | `pwsh -Command 'Invoke-ScriptAnalyzer -Path . -Recurse -Severity Warning,Error'` |

## CI gap reminder

Cross-target clippy catches many compile-time Windows failures from Linux, but it
does not validate runtime Windows behavior. Keep Windows CI (or a Windows VM)
for runtime confirmation.

## Common failure classes

- missing/stale `#[cfg(...)]` gating
- platform-only imports referenced from non-gated code
- hardcoded path separators
- wrong executable suffix assumptions
