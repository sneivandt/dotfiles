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
- Home links must never be UAC-delegated: over-the-shoulder authentication can
  change `USERPROFILE` and HKCU. Recheck capability in `run()` after the
  ordering-only Developer Mode task. Without capability, skip pending file
  links as unmet work while processing directory links and already-correct links.
  Directory links can fall back to junctions.
- Dry-run never requests elevation.

## Elevation

- Keep the parent process unprivileged.
- Delegate only machine-scoped tasks whose current state reports
  `needs_elevation()`. The application allowlist currently permits only
  `developer-mode`; user-profile tasks, including HKCU registry work, must
  remain in the parent. Desired state already satisfied must not prompt.
- The child uses exact selectors, resolved absolute root/overlay paths and an
  explicit working directory, and cannot recurse. Validate explicit or inherited
  elevated-child scope before startup and again before task execution; do not
  allow dependency expansion to introduce user-scoped work.
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
[Platform coverage parity](../../../docs/content/development/testing.md#platform-coverage-parity)
after Windows-sensitive changes.
