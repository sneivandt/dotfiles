---
name: cross-platform-verification
description: >
  How to verify Rust and shell-wrapper changes work on both Linux and Windows
  without waiting for CI. Use after any change to cli/src/, dotfiles.sh, or
  dotfiles.ps1 — especially when adding cfg gates, touching paths, symlinks,
  registry, or platform-specific imports.
---

# Cross-Platform Verification

The CI matrix runs Linux and Windows in parallel; many failures only surface on
the platform you didn't develop on. Run these local checks before pushing.

## Required local checks (Rust changes)

```sh
cd cli
cargo fmt --check
cargo clippy --all-targets -- -D warnings                    # host
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test
```

The cross-target clippy run is the key step that catches Windows CI failures
from a Linux box. The pre-commit hook (`hooks/check-rust.sh`) runs all four
automatically when `.rs` files are staged, and skips the cross step with a
notice if the toolchain is missing.

### Toolchain setup (one-time)

```sh
rustup target add x86_64-pc-windows-gnu
# Arch:    sudo pacman -S mingw-w64-gcc
# Debian:  sudo apt install gcc-mingw-w64-x86-64
```

`git2`/`libssh2`/`openssl-sys` build with bundled C sources via the existing
`Cargo.toml`, so no extra system libraries are needed.

### What cross-clippy catches

- Missing `#[cfg(windows)]` arm or stale Linux-only import after refactor
- `winreg`/`windows`-only types referenced from non-gated code
- `cfg!(target_os = "windows")` branches that don't compile
- `unused_imports` / `dead_code` lints that only fire on the other platform
- Type mismatches in `MetadataExt`, `FromRawHandle`, etc.

### What it does NOT catch

- Runtime behaviour differences (executable extension, path separators in
  string literals, `\r\n` vs `\n` parsing, Developer-Mode symlink failure).
  These still need the Windows CI job or a Windows VM.

## Required local checks (shell wrapper changes)

| Change touches      | Run locally                                                  |
|---------------------|--------------------------------------------------------------|
| `dotfiles.sh`, `*.sh` | `shellcheck --severity=warning --shell=sh dotfiles.sh ...` |
| `dotfiles.ps1`, `*.ps1`, `*.psm1` | `pwsh -Command 'Invoke-ScriptAnalyzer -Path . -Recurse -Severity Warning,Error'` |

Both run automatically in the pre-commit hook for staged files when the
respective tool is installed.

## Common cross-platform failure classes in this repo

| Failure                                                           | Prevention                                            |
|-------------------------------------------------------------------|-------------------------------------------------------|
| Code under one `#[cfg(...)]` arm references items missing in the other | Cross-target clippy                                   |
| Directory symlink misdetected on Windows                          | Use `MetadataExt::file_attributes() & 0x10` (see `windows-specific-patterns`) |
| Hardcoded `/` in paths                                            | Use `Path::join` / `PathBuf::push`, never string concat |
| `dotfiles` vs `dotfiles.exe` in scripts                           | Use `std::env::consts::EXE_SUFFIX` in Rust; in scripts, derive from `$OS` |
| Line endings on shell scripts checked out on Windows              | `.gitattributes` already enforces LF for `*.sh`       |
| Calling `chmod`/`systemctl` unconditionally                       | Gate with `ctx.platform.is_linux()` in `should_run()` |
| Unconditional `winreg` use                                        | Gate task with `ctx.platform.is_windows()`            |

## Pre-signoff checklist for agents

After modifying anything in `cli/src/`, `dotfiles.sh`, or `dotfiles.ps1`:

1. Run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.
2. Run `cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings`.
   If the target isn't installed and installing it isn't appropriate, state
   so explicitly in the response so the user knows Windows CI risk remains.
3. Run `cargo test`.
4. For shell-wrapper changes, run `shellcheck` and/or `Invoke-ScriptAnalyzer`.
5. Review the diff for any of the failure classes in the table above.

## Related

- **`windows-specific-patterns`** skill — Windows-only code patterns
- **`shell-patterns`** skill — POSIX shell wrapper conventions
- **`testing-patterns`** skill — full test strategy
- **`ci-cd-patterns`** skill — CI job structure these checks mirror
