---
name: cross-platform-verification
description: >
  Use after changes to cli/src/, dotfiles.sh, or dotfiles.ps1 to select the
  canonical Linux/Windows checks. This is validation guidance, not an
  implementation-pattern skill.
---

# Cross-Platform Verification

Use the repository runners so local checks match CI:

```sh
sh .github/workflows/scripts/linux/check.sh
```

```powershell
pwsh -File .github\workflows\scripts\windows\Check.ps1
```

During iteration, pass only the needed stages. Use `--list` to inspect available
stages. Run the shell stage for `*.sh` and the PowerShell stage for `*.ps1` or
`*.psm1`.

For Rust changes that can break Windows compilation, also run:

```sh
cd cli
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
```

If the target/toolchain is unavailable, report the omitted check explicitly.
Cross-target compilation does not prove Windows runtime behavior; retain Windows
CI or VM coverage for registry, elevation, symlinks, and path handling.

Typical failures are stale `cfg` gates, platform-only imports in shared code,
hardcoded separators, and executable-suffix assumptions.
