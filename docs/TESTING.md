# Testing

This document describes the testing infrastructure and procedures for the dotfiles project.

## Test Suite

The project uses Rust's built-in test framework. All tests are run with `cargo test`.

### Choosing the Right Test Command

- **`cargo test --manifest-path cli/Cargo.toml`** runs the Rust unit, integration,
  and snapshot-backed task list tests.
- **`./dotfiles.sh test`** runs the dotfiles configuration validation command
  against the checked-out repository and reports config drift, missing files, or
  invalid local APM plugin packages.

Use `cargo test` when changing Rust code or test fixtures, and use
`./dotfiles.sh test` when changing files under `conf/`, `symlinks/`, `hooks/`, or
wrapper scripts.

### Cross-Platform Rust Check

After any Rust change, also run Clippy against the Windows target to catch
platform-gated imports, `#[cfg(windows)]` arms, and `winreg` references before
CI:

```bash
rustup target add x86_64-pc-windows-gnu
cargo clippy --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings
```

The target also requires a mingw-w64 GCC toolchain on Linux. If it is not
installed, the pre-commit hook skips this check with a notice unless full mode
is explicitly enabled.

### Running Tests

```bash
# Run all tests (unit + integration)
cargo test --manifest-path cli/Cargo.toml

# Run tests with output
cargo test --manifest-path cli/Cargo.toml -- --nocapture

# Run a specific test
cargo test --manifest-path cli/Cargo.toml -- test_name
```

### Test Organization

#### 1. Unit Tests (`#[cfg(test)]` modules)

Unit tests live alongside the code they test in `cli/src/`. Small modules keep
tests inline; larger test modules can live in sibling files such as `tests.rs`,
or in a grouped test folder such as `resources/tests/<resource>.rs`, and are
included from the production module with `#[cfg(test)]` plus `#[path = "..."]`.
Examples:
- `platform.rs` — Platform detection and category exclusion logic
- `cli.rs` — CLI argument parsing and command structure
- `config/toml_loader.rs` — TOML file parsing
- `tasks/<domain>/*.rs` — Task `should_run` logic and helper functions

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn my_unit_test() {
        // test implementation
    }
}
```

Task tests use context builder helpers defined in `tasks/mod.rs` (available in `#[cfg(test)]` scope):
- `make_linux_context(config)`, `make_arch_context(config)`, `make_windows_context(config)`
- `make_platform_context(config, os, is_arch)`, `make_platform_context_with_which(...)`
- `empty_config(root)` — creates a `Config` with all empty vecs

For tasks that use their own `fs_ops` field (e.g., `InstallGitHooks`), inject a
mockall-generated `MockFileSystemOps` via the task's own constructor, e.g.
`InstallGitHooks::with_fs_ops(Arc::new(mock))`, to avoid touching the real filesystem.

#### 2. Integration Tests (`cli/tests/`)

Separate test binaries in `cli/tests/` test the full command output:
- `install_command.rs` — verifies the install task list
- `uninstall_command.rs` — verifies the uninstall task list
- `test_command.rs` — verifies config validation

Integration tests use helpers from `cli/tests/common/mod.rs`:
- `IntegrationTestContext::new()` — sets up a temp-dir-backed repo clone
- `TestContextBuilder` — builder for custom repo layouts

##### Writing New Integration Tests

Use `IntegrationTestContext` for tests that need an isolated repository:

```rust
mod common;

#[test]
fn my_test() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    // ... assertions ...
}
```

`TestContextBuilder` lets you override individual config files and create
source files before building the context:

```rust
#[test]
fn test_with_custom_symlinks() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
        .with_symlink_source("bashrc")   // creates the source file on disk
        .build();

    let config = ctx.load_config("base");
    assert_eq!(config.symlinks.len(), 1);
}
```

##### Fixture Files

Reusable TOML config files live in `cli/tests/fixtures/`. Use them with
`TestContextBuilder::with_config_file` to avoid duplicating inline strings:

```rust
#[test]
fn test_with_base_fixture() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", include_str!("fixtures/base_profile.toml"))
        .with_symlink_source("bashrc")
        .build();

    let config = ctx.load_config("base");
    assert_eq!(config.symlinks.len(), 1);
}
```

| File | Contents | Used by |
| --- | --- | --- |
| `fixtures/base_profile.toml` | `[base]` section with a single `bashrc` symlink | symlink validation tests |
| `fixtures/desktop_profile.toml` | `[base]` + `[desktop]` sections with one symlink each | desktop profile loading test |

#### 3. Snapshot Tests

Task list tests use the `insta` crate for snapshot assertions. Snapshot files live in
`cli/tests/snapshots/`. Update them when task lists change:

```bash
INSTA_UPDATE=unseen cargo test  # auto-accept new/changed snapshots
cargo insta review              # interactive review
```

Always commit `.snap` files together with the code changes that cause them.

#### 4. Configuration Validation

The `test` subcommand validates all configuration files at runtime:

```bash
# Via the binary directly
./dotfiles.sh test

# Or in dev mode
./dotfiles.sh --build test
```

This runs the same validation tasks as `commands/test.rs`, covering:
- configuration warnings reported by `Config::validate()`
- symlink source file existence
- required config file presence
- `symlinks.toml` / `manifest.toml` section drift
- shell and `PowerShell` script linting when the corresponding tools are installed

## Manual Testing

### Dry-Run Mode

Preview changes without making modifications:

```bash
./dotfiles.sh --build install -p desktop -d
# Shows what would be done without making changes
# Verbose mode is automatically enabled
```

### Profile Testing

Test different profiles to ensure sparse checkout and configuration work correctly:

```bash
# Test base profile
./dotfiles.sh --build install -p base -d

# Test desktop profile
./dotfiles.sh --build install -p desktop -d

# Test desktop profile (on Windows)
.\dotfiles.ps1 --build install -p desktop -d
```

## Continuous Integration

### GitHub Actions CI (`.github/workflows/ci.yml`)

Runs automatically on pull requests with the following jobs:

| Job | Matrix | Purpose |
| --- | --- | --- |
| `rust-fmt` | — | Rust format check (`cargo fmt --check`) |
| `lint` | ShellCheck, PSScriptAnalyzer | Static analysis for shell and PowerShell scripts |
| `validate-config` | — | 6 config checks: TOML syntax, file references, category consistency, empty sections |
| `audit` | — | Cargo security audit (vulnerability scan via `cargo-audit`) |
| `deny` | — | Cargo deny: license and advisory policy check |
| `build-linux` | — | Linux release build + Clippy + unit/integration tests |
| `build-windows` | — | Windows release build + Clippy + unit/integration tests |
| `integration-linux` | base, desktop | Dry-run install and config validation per profile on Linux |
| `integration-windows` | base, desktop | Dry-run install and config validation per profile on Windows |
| `test-install-uninstall` | — | Install/uninstall round-trip test (Linux) |
| `test-install-uninstall-windows` | — | Install/uninstall round-trip test (Windows) |
| `test-applications` | git, zsh, vim, nvim | Application-specific behavior tests |
| `test-git-hooks` | — | Pre-commit sensitive data detection |
| `test-shell-wrapper-linux` | — | Linux wrapper script (`dotfiles.sh`) validation |
| `test-shell-wrapper-windows` | — | Windows wrapper script (`dotfiles.ps1`) validation |

### Release Pipeline (`.github/workflows/release.yml`)

Triggers automatically when the CI workflow completes successfully on `main`:
1. Builds Linux (x86_64, aarch64) and Windows (x86_64) release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with versioned tag

### Running CI Checks Locally

Replicate the full CI validation locally:

```bash
# Formatting
cargo fmt --check --manifest-path cli/Cargo.toml

# Linting
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings

# Tests
cargo test --manifest-path cli/Cargo.toml

# Release build
cargo build --release --manifest-path cli/Cargo.toml

# Integration: dry-run per profile
./dotfiles.sh --build install -p base -d
./dotfiles.sh --build install -p desktop -d

# Shell wrapper lint
shellcheck --severity=warning --shell=sh --exclude=SC1090,SC1091,SC3043,SC2154 --enable=avoid-nullary-conditions dotfiles.sh install.sh
```

## Best Practices

When contributing changes:

1. **Run tests before committing:**
   ```bash
   cargo test --manifest-path cli/Cargo.toml
   ```

2. **Run all lints, including Windows-target Clippy for Rust changes:**
   ```bash
   cargo fmt --check --manifest-path cli/Cargo.toml
   cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
   cargo clippy --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings
   ```

3. **Run configuration validation when changing config, symlinks, hooks, or wrappers:**
   ```bash
   ./dotfiles.sh test
   ```

4. **Test with dry-run:**
   ```bash
   ./dotfiles.sh --build install -p base -d
   ```

5. **Test affected profiles:**
   - If modifying base configuration, test `base` profile
   - If modifying desktop items, test `desktop` profile
   - Platform categories (arch, windows) are auto-detected and tested in CI
   - See [Profile System](PROFILES.md) for profile details

6. **Verify no trailing whitespace** in all files

## Troubleshooting Tests

### Cargo Test Failures
- Review test output for specific assertions
- Run with `-- --nocapture` to see println output
- Check that `cli/Cargo.lock` is committed

### Clippy Warnings
- All warnings are treated as errors (`-D warnings`)
- Fix the warning or add a targeted `#[allow(..., reason = "...")]` explaining why

### Configuration Validation Failures
- Check TOML file syntax
- Ensure section headers use proper format:
  - Profile names in `profiles.toml`: `[profile-name]`
  - Section names in other files: `[category]` or `[category-another]`
- Verify no trailing whitespace

### Integration Test Failures
- Ensure the binary builds: `cargo build --release --manifest-path cli/Cargo.toml`
- Check that `conf/` files are valid
- Run the failing profile manually with `-d` (dry-run) and `-v` (verbose)
- Add `RUST_BACKTRACE=1` before `cargo test` for full stack traces on failures
- `IntegrationTestContext` uses `tempfile::TempDir`, which is automatically
  cleaned up when dropped. Log the temp directory path if you need to inspect
  it during debugging.
- Snapshot mismatches show a diff: the left side is the stored snapshot, the
  right side is the actual output. If the change is expected, update the
  snapshot as described above.

## Next read

- [Contributing Guide](CONTRIBUTING.md) - Development workflow and PR expectations
- [Architecture](ARCHITECTURE.md) - Implementation details
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Git Hooks](HOOKS.md) - Local pre-commit checks and full-mode validation
- [Troubleshooting](TROUBLESHOOTING.md) - Diagnosing failed installs or tests
