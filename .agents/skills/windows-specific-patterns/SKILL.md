---
name: windows-specific-patterns
description: >
  Use for Windows registry resources, symlink capability/junction behavior,
  elevation delegation, Windows-only cfg gates, or Windows-specific wrapper
  behavior beyond shared bootstrap. Not for ordinary wrapper parity or checks.
---

# Windows-Specific Patterns

Prefer capability methods. Use a direct Windows check only when the behavior is
inherently Windows-specific and has no narrower capability.

## Registry

- Registry desired state comes from `conf/registry.toml`.
- Supported paths are HKCU-only; entries are not profile-filtered.
- Preserve the shared batch state query before resource convergence.
- Keep native access in `RegistryResource`, not in the task.

## Symlinks

- Use `infra::fs::is_dir_like()` for classification and
  `create_native_symlink()` for native dispatch.
- Resource code may fall back to junctions for directory links.
- Check capability before mutation. Developer Mode permits unprivileged links;
  otherwise the applicable symlink task requires elevation.
- Dry-run never requests elevation.

## Elevation

- Keep the parent process unprivileged.
- Delegate only tasks whose current state reports `needs_elevation()`.
- The elevation-capable tasks are `EnableDeveloperMode` and `InstallSymlinks`;
  neither should prompt once its desired state is already satisfied.
- The child uses explicit selectors and cannot recurse.
- Never prompt in CI or a non-interactive session.
- If elevation is unavailable, skip elevating tasks and their transitive
  dependents. If the child ran successfully, continue dependents normally.
- Build child arguments in a pure, platform-independent function for tests.

## PowerShell wrapper

`dotfiles.ps1` owns bootstrap, checksum/provenance verification, build mode, and
argument forwarding only. Use `$ErrorActionPreference = 'Stop'` and `Join-Path`.
Task behavior belongs in Rust.

Use the native-runtime caveats and checks under
[Platform coverage parity](../../../docs/TESTING.md#platform-coverage-parity)
after Windows-sensitive changes.
