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

## Canonical local sequence

Run every gating check the way CI runs it:

```sh
sh .github/workflows/scripts/linux/check.sh
```

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

Both use the `ci` Cargo profile, report `SKIP` for stages whose tool is missing,
exit 1 on failure and 2 on an unknown stage, and accept explicit stage names
(`fmt clippy test config shell powershell audit deny`, plus `msrv` via `--all`).
Use `--list` to see the stages.

While iterating, run the narrowest stage that covers the change, for example
`check.sh fmt clippy`.

The runner does not cross-compile. After a change that could break the Windows
build, still run:

```sh
cd cli
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
```

If the Windows target/toolchain is unavailable and installing it is not
appropriate, report that explicitly.

## Shell wrapper checks

| Change touches | Run |
|---|---|
| `dotfiles.sh`, `*.sh` | `check.sh shell` (ShellCheck over the same file set CI lints) |
| `dotfiles.ps1`, `*.ps1`, `*.psm1` | `check.sh powershell` (PSScriptAnalyzer) |

Use the runner rather than invoking the linters directly, so the file list and
severity flags stay identical to CI.

Console output from the wrappers should match the CLI's style rather than
free-form progress prose; see `logging-patterns` for the rule and
`shell-patterns` for the concrete wrapper example.

## CI gap reminder

Cross-target clippy catches many compile-time Windows failures from Linux, but it
does not validate runtime Windows behavior. Keep Windows CI (or a Windows VM)
for runtime confirmation.

Windows runtime coverage in CI is deliberately narrower than Linux, because
`zsh`, `vim`, and `nvim` are excluded from the Windows profile. Git *is*
managed on Windows, and `test-applications-windows` asserts the effective
configuration — including the Windows-only `core.autocrlf = true` override that
only lands when the `symlinks/config/git/windows` include is symlinked. See the
platform coverage parity table in `docs/TESTING.md`.

## Common failure classes

- missing/stale `#[cfg(...)]` gating
- platform-only imports referenced from non-gated code
- hardcoded path separators
- wrong executable suffix assumptions
