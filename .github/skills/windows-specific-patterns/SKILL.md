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
- **Registry Configuration**: Managed via `conf/registry.ini` (no Linux equivalent)
- **Fixed Profile**: Windows uses `base` or `desktop` profiles — the `windows` category is auto-detected
- **Symlinks**: Require Developer Mode or admin privileges
- **No systemd/chmod**: Unit and file-mode tasks skip on Windows

## Registry Configuration

### conf/registry.ini

Registry settings use INI format with registry paths as section headers:

```ini
# Section = Registry path (PowerShell format)
[HKCU:\Console]
WindowSize = 0x00200078
ScreenBufferSize = 0x0BB80078
FaceName = Cascadia Mono

[HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer]
EnableAutoTray = 0
```

**Format rules:**
- Section headers are registry paths (`HKCU:\`, `HKLM:\`)
- Entries are `ValueName = Value` (key-value, unlike other INI files which are list-based)
- Values can be hex (`0x...`), numeric, or string
- No profile filtering — all entries are applied on Windows

### Registry Rust Task

The registry task in `cli/src/tasks/registry.rs`:
1. Parses `conf/registry.ini` into path → key/value maps
2. For each entry, compares the current registry value with the desired value
3. Skips entries that already match (idempotent)
4. In dry-run mode, logs what would change without writing

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
| File permissions | `chmod.ini` | N/A (skipped) |
| Registry | N/A | `registry.ini` |
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
3. **`registry.ini` uses `key = value`** format — unlike other list-based INI files
4. **No profile sections** in `registry.ini` — all settings always apply
5. **Test symlink capability** before attempting creation; report Developer Mode hint on failure
6. **Use `Join-Path`** in `dotfiles.ps1` — never hardcode backslashes

## Related

- **`ini-configuration`** skill — INI format rules
- **`error-handling-patterns`** skill — Idempotency and dry-run
- **`docs/WINDOWS.md`** — User-facing Windows guide
- **`docs/CONFIGURATION.md`** — Configuration reference
