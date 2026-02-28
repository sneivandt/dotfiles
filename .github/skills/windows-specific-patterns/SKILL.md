---
name: windows-specific-patterns
description: >
  Windows-specific implementation patterns and considerations beyond general PowerShell patterns.
  Use when working with Windows features, registry, admin privileges, or Windows-specific architecture.
metadata:
  author: sneivandt
  version: "2.0"
---

# Windows-Specific Patterns

Windows-specific implementation patterns in the Rust core engine and `dotfiles.ps1` wrapper.

## Overview

The project uses a single Rust binary on both platforms. The `dotfiles.ps1` wrapper downloads (or builds) the binary the same way `dotfiles.sh` does. Windows-specific behaviour lives in the Rust task implementations, gated by `ctx.platform.is_windows()`.

Key differences from Linux:
- **Registry Configuration**: Managed via `conf/registry.toml` (no Linux equivalent)
- **Fixed Profile**: Windows uses `base` or `desktop` profiles — the `windows` category is auto-detected
- **Symlinks**: Require Developer Mode or admin privileges
- **No systemd/chmod**: Unit and file-mode tasks skip on Windows

## Registry Configuration

### conf/registry.toml

Registry settings use TOML format with logical section names and a `path` key:

```toml
# Section = logical name, path = Registry path (PowerShell format)
[console]
path = 'HKCU:\Console'

[console.values]
WindowSize = 0x00200078
ScreenBufferSize = 0x0BB80078
FaceName = "Cascadia Mono"

[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

**Format rules:**
- Section headers are logical names with a `path` key for the registry path (`HKCU:\`, `HKLM:\`)
- Values are defined in a `[section.values]` subtable as `ValueName = Value` pairs
- Values can be hex (`0x...`), numeric, or string
- No profile filtering — all entries are applied on Windows

### Registry Rust Task

The registry task in `cli/src/tasks/registry.rs`:
1. Parses `conf/registry.toml` into path → key/value maps
2. Batch-checks all current values via `batch_check_values(&resources)`
3. Compares each entry's current value with the desired value
4. Skips entries that already match (idempotent)
5. In dry-run mode, logs what would change without writing

`RegistryResource` uses the `winreg` crate for native registry access (no executor needed):
```rust
let resources: Vec<_> = ctx.config_read().registry.iter()
    .map(RegistryResource::from_entry)
    .collect();
let cached = batch_check_values(&resources)?;
```

## Symlink Differences on Windows

### MetadataExt Workaround

Rust's `symlink_metadata().is_dir()` returns `false` for directory symlinks on Windows. Use the raw file attributes instead:

```rust
#[cfg(windows)]
fn is_dir_like(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    match path.symlink_metadata() {
        Ok(m) => m.file_attributes() & 0x10 != 0, // FILE_ATTRIBUTE_DIRECTORY
        Err(_) => false,
    }
}
```

### Developer Mode and Admin Privileges

- **Developer Mode** (Windows 10+): Allows symlink creation without elevation
- **Without Developer Mode**: Symlinks require running as Administrator
- The Rust binary checks for symlink capability and reports a clear error if neither condition is met
- Dry-run mode never requires elevation

## Cross-Platform Feature Parity

| Feature | Linux | Windows |
|---|---|---|
| Package management | `pacman`/`paru` | `winget` |
| Symlinks | `std::os::unix::fs::symlink` | `std::os::windows::fs::symlink_file/dir` |
| Service management | `systemctl` | N/A (skipped) |
| File permissions | `chmod.toml` | N/A (skipped) |
| Registry | N/A | `registry.toml` |
| Profile | User-selected (`base`/`desktop`) | User-selected (`base`/`desktop`) |
| Sparse checkout | Profile-filtered | Profile-filtered |

## Shell Wrapper (`dotfiles.ps1`)

The PowerShell wrapper mirrors `dotfiles.sh`:
- Downloads a pre-built binary from GitHub Releases (or `cargo build` with `--build`)
- Verifies SHA256 checksum after download
- Falls back to an existing binary when GitHub is unreachable
- Uses `$ErrorActionPreference = 'Stop'` for fail-fast behaviour
- Caches the installed version in `bin/.dotfiles-version-cache`

## Rules

1. **Gate Windows-only tasks** with `ctx.platform.is_windows()` in `should_run()`
2. **Use `MetadataExt::file_attributes()`** for directory-symlink checks on Windows
3. **`registry.toml` uses logical section names** with `path` key and `[section.values]` subtable
4. **No profile sections** in `registry.toml` — all settings always apply
5. **Test symlink capability** before attempting creation; report Developer Mode hint on failure
6. **Use `Join-Path`** in `dotfiles.ps1` — never hardcode backslashes

## Related

- **`toml-configuration`** skill — TOML format rules
- **`error-handling-patterns`** skill — Idempotency and dry-run
- **`docs/WINDOWS.md`** — User-facing Windows guide
- **`docs/CONFIGURATION.md`** — Configuration reference
