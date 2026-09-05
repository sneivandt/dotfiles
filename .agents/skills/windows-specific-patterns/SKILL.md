---
name: windows-specific-patterns
description: >
  Use for Windows registry resources, symlink capability/junction behavior,
  elevation delegation, Windows-only cfg gates, or Windows-specific wrapper
  behavior beyond shared bootstrap. Not for ordinary wrapper parity or checks.
---

# Windows-Specific Patterns

Prefer capability methods. Use a direct Windows check only when the behavior is
inherently Windows-specific and has no narrower capability, within the platform
abstraction. `#[cfg(windows)]` gates platform-only implementations/imports;
`cfg!(windows)` is a runtime boolean and does not hide unavailable APIs from the
other target. Preserve the boundaries enforced by
[`domain_boundaries`](../../../cli/tests/domain_boundaries.rs).

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
- Check current task metadata and assessments rather than maintaining a second
  inventory of elevation-capable tasks. Developer Mode and symlink tasks must
  not prompt once their desired state is already satisfied.
- The child uses explicit selectors and cannot recurse.
- Never prompt in CI or a non-interactive session.
- If elevation is unavailable, skip elevating tasks and their transitive
  dependents. If the child ran successfully, continue dependents normally.
- Build child arguments in a pure, platform-independent function for tests.

## PowerShell wrapper

`dotfiles.ps1` owns bootstrap, checksum/provenance verification, build mode, and
argument forwarding only. Use `$ErrorActionPreference = 'Stop'` and `Join-Path`.
Check native executable exit codes explicitly; PowerShell's error preference
does not by itself make every non-zero native exit terminating. Task behavior
belongs in Rust.

Cover paths with spaces, drive roots, UNC/extended prefixes, and case differences
where the changed code handles paths. Do not approximate Windows path rules by
testing only Unix-looking strings on the host.

Use the native-runtime caveats and checks under
[Platform coverage parity](../../../docs/TESTING.md#platform-coverage-parity)
after Windows-sensitive changes.
