---
name: windows-specific-patterns
description: >
  Use for Windows registry resources, symlink capability/junction behavior,
  elevation delegation, or Windows-only cfg gates in Rust. Not for wrapper
  bootstrap/parity (use self-update).
---

# Windows-Specific Patterns

Preserve the platform/domain split enforced by
[`domain_boundaries`](../../../cli/tests/domain_boundaries.rs).

## Registry

- `conf/registry.toml` supports HKCU paths only (not profile-filtered); keep
  native access in `RegistryResource` with a single shared batch query before
  convergence.

## Symlinks

- Use `infra::fs::is_dir_like()` and `create_native_symlink()`.
- Never UAC-delegate home symlinks: over-the-shoulder elevation can switch
  `USERPROFILE` and HKCU to another account.
- Recheck symlink capability in `run()` after the ordering-only Developer Mode
  task. Without capability, skip pending file links as unmet while still
  processing already-correct links and directory links (which fall back to
  junctions). Dry-run never requests elevation.

## Elevation

- Keep the parent process unprivileged and delegate only machine-scoped tasks
  reporting `needs_elevation()`. The application allowlist currently permits
  only `developer-mode`; user-profile and HKCU tasks stay in the parent, and
  already-satisfied state must not prompt.
- Never prompt in CI or non-interactive sessions. When elevation is unavailable,
  skip elevating tasks and their transitive dependents.
- The elevated child uses exact selectors, resolved absolute root/overlay paths,
  an explicit working directory, and cannot recurse. Validate elevated-child
  scope both before startup and before task execution so dependency expansion
  cannot pull in user-scoped work. Build child args in a pure helper for tests.

Cover spaces, drive roots, UNC/extended prefixes, and case differences when
changing Windows path code, and check
[Platform coverage parity](../../../docs/content/development/testing.md#platform-coverage-parity).
