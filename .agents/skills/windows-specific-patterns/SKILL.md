---
name: windows-specific-patterns
description: >
  Windows-specific patterns for this dotfiles repo. Use when changing registry
  resources, Windows symlink capability, elevation behavior, platform-specific
  Rust code, or the dotfiles.ps1 wrapper.
---

# Windows-Specific Patterns

Windows-specific implementation patterns in the Rust core engine and `dotfiles.ps1` wrapper.

## Overview

The project uses a single Rust binary on both platforms. The `dotfiles.ps1`
wrapper downloads or builds it the same way `dotfiles.sh` does. Prefer capability
methods for applicability; use a direct Windows platform check only when the
behavior is inherently Windows-specific and has no narrower capability.

Key differences from Linux:
- **Registry Configuration**: Managed via `conf/registry.toml` (no Linux equivalent)
- **Fixed Profile**: Windows uses `base` or `desktop` profiles — the `windows` category is auto-detected
- **Symlinks**: Require Developer Mode or admin privileges
- **No systemd/chmod**: Unit and file-mode tasks skip on Windows

## Registry Configuration

### conf/registry.toml

Registry settings use TOML format with logical section names and a `path` key:

```toml
[console]
path = 'HKCU:\Console'

[console.values]
FaceName = "Cascadia Mono"
```

**Format rules:**
- Section headers are logical names with an `HKCU:\` registry `path`
- Other hives such as `HKLM:\` and `HKCR:\` are rejected
- Values are defined in a `[section.values]` subtable as `ValueName = Value` pairs
- Values can be hex (`0x...`), numeric, or string
- No profile filtering — all entries are applied on Windows

### Registry Rust Task

The registry task in `cli/src/domains/system/registry.rs`:
1. Parses `conf/registry.toml` into path → key/value maps
2. Batch-checks all current values via `batch_check_values(&resources)`
3. Compares each entry's current value with the desired value
4. Skips entries that already match (idempotent)
5. In dry-run mode, logs what would change without writing

`RegistryResource` uses `winreg` for native registry access. Preserve the
single batch state query rather than checking values individually.

## Symlink Differences on Windows

### MetadataExt Workaround

Rust's `symlink_metadata().is_dir()` is not reliable for Windows directory
symlinks. Use `MetadataExt::file_attributes()` and the directory attribute, as
the existing implementation does.

### Developer Mode and Admin Privileges

- **Developer Mode** (Windows 10+): Allows symlink creation without elevation
- **Without Developer Mode**: Symlinks require running as Administrator
- The Rust binary checks for symlink capability and reports a clear error if neither condition is met
- Dry-run mode never requires elevation

## Elevation

Windows elevation is scoped the same way Unix `sudo` priming is: the main
process stays unprivileged and only the tasks whose `needs_elevation()` returns
`true` are delegated to one short-lived elevated child run, launched with
`--only <selectors> --no-parallel` and a hidden marker flag so the child cannot
recurse.

Only two tasks can ever need it — `EnableDeveloperMode` (first run only) and
`InstallSymlinks` (only when Developer Mode is off). Every other Windows task is
user-scoped.

Rules:

- Never re-launch the whole process elevated, and never gate elevation on the
  command instead of task state.
- Never prompt in a non-interactive or CI session; degrade instead.
- When elevation is declined or unavailable, skip the elevating tasks **and their
  transitive dependents**. Dropping a task from the slice is not the same as
  failing it: the resolved graph ignores dependencies absent from the slice, so
  dependents must be skipped explicitly.
- Delegation is not degradation. When the elevated child actually ran the tasks,
  the prerequisite happened, so dependents must still run.
- Build the child command line in a pure function so it is testable off Windows.

## Shell Wrapper (`dotfiles.ps1`)

The PowerShell wrapper mirrors `dotfiles.sh`. Keep it limited to bootstrap,
checksum verification, build mode, and argument forwarding; use
`$ErrorActionPreference = 'Stop'` and `Join-Path`.

## Rules

1. Prefer capability gates; use a platform check only when no narrower
   capability represents the requirement.
2. Registry paths remain HKCU-only and unfiltered by profile.
3. Test symlink capability before mutation and report the Developer Mode hint
   when unavailable.
4. Elevation stays scoped to the tasks that declare it; the run degrades rather
   than aborting when it is unavailable.
