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

The target also requires a mingw-w64 GCC toolchain on Linux. The pre-commit hook
runs this check only when full mode is explicitly enabled, and then skips it
with a notice if the toolchain is unavailable.

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
with related resource suites separated by nested modules inside that file.
Standard sibling module wiring is preferred; `#[path]` is reserved for
established externalized test layouts. Domain-root task entry points put
tests-only externalized modules under `domains/<domain>/tests/` rather than
creating a same-named feature folder solely for tests.
Examples:
- `infra/platform.rs` — Platform detection and category exclusion logic
- `app/cli.rs` — CLI argument parsing and command structure
- `infra/config/toml_loader.rs` — TOML file parsing
- `domains/<domain>/*.rs` — Task applicability and helper functions

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

Task tests use context builder helpers from `app/test_helpers.rs` (available in
`#[cfg(test)]` scope):
- `make_linux_context(config)`, `make_arch_context(config)`, `make_windows_context(config)`
- `ContextBuilder`, `make_platform_context_with_which(...)`
- `empty_config(root)` — creates a `Config` with all empty vecs

For tasks that use their own `fs_ops` field (e.g., `InstallGitHooks`), inject a
mockall-generated `MockFileSystemOps` via the task's own constructor, e.g.
`InstallGitHooks::with_fs_ops(Arc::new(mock))`, to avoid touching the real filesystem.

#### 2. Integration Tests (`cli/tests/`)

Separate test binaries in `cli/tests/` cover cross-module contracts:

| Test binary | Coverage |
|---|---|
| `behavioral_ci.rs` | Profile/platform filtering, filesystem outcomes, idempotency, and emitted external commands |
| `config_drift.rs` | Synchronization between conditional symlinks and sparse-checkout manifest coverage |
| `domain_boundaries.rs` | Domain import-boundary architecture rules |
| `e2e_apply.rs` | Hermetic, non-dry-run config-to-filesystem apply pipeline |
| `install_command.rs` | Install task list, selectors, and dependency graph |
| `task_execution.rs` | Real task execution, dry-run safety, and isolated filesystem/config outcomes |
| `test_command.rs` | Configuration loading and validation diagnostics |
| `uninstall_command.rs` | Uninstall task list structure and naming |

Integration tests use helpers from `cli/tests/common/mod.rs`:
- `IntegrationTestContext::new()` — sets up a temp-dir-backed repo clone
- `TestContextBuilder` — builder for custom repo layouts
- `ExecutionContext` — owns an isolated home, config store, context, and logger

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

This runs the same validation tasks as `app/commands/test.rs`, covering:
- structured configuration diagnostics reported by `Config::validate()`
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

Runs automatically on pull requests with a lightweight `classify-changes` job
first. That job uses the actual Git diff to skip expensive Rust and
cross-platform jobs for documentation-only changes (including `.agents/`
documentation) while still running the relevant validation for workflow, source,
dependency, configuration, wrapper, and hook changes.

The following gating jobs may run depending on the changed paths:

| Job | Matrix | Purpose |
| --- | --- | --- |
| `rust-fmt` | — | Rust format check (`cargo fmt --check`) |
| `lint` | ShellCheck, PSScriptAnalyzer | Static analysis for shell and PowerShell scripts |
| `validate-config` | — | Manifest/profile consistency, file references, TOML whitespace, categories, empty sections, and fullscreen Waybar rules |
| `audit` | — | Cargo security audit (vulnerability scan via `cargo-audit`) |
| `deny` | — | Cargo deny: license and advisory policy check |
| `build-linux` | — | Linux CI-profile build + Clippy + unit/integration tests |
| `msrv` | — | Compatibility check against the minimum supported Rust version (1.91) |
| `build-windows` | — | Windows CI-profile build + Clippy + unit/integration tests |
| `integration-linux` | base, desktop | Dry-run install and config validation per profile on Linux |
| `integration-windows` | base, desktop | Dry-run install and config validation per profile on Windows |
| `test-install-uninstall` | — | Install/uninstall round-trip test (Linux) |
| `test-install-uninstall-windows` | — | Install/uninstall round-trip test (Windows) |
| `test-applications` | git, zsh, vim, nvim | Application-specific behavior tests |
| `test-git-hooks` | — | Pre-commit sensitive data detection |
| `test-shell-wrapper-linux` | — | Linux wrapper script (`dotfiles.sh`) validation |
| `test-shell-wrapper-windows` | — | Windows wrapper script (`dotfiles.ps1`) validation |

The separate `coverage` job is informational and does not gate CI success. The
workflow is authoritative for the current job definitions.

`All CI Checks Passed` remains the single dependable required check. It runs on
`if: always()`, fails when any required job fails or is cancelled, and treats
intentional path-filter skips as success.

### Release Pipeline (`.github/workflows/release.yml`)

Triggers automatically when the CI workflow completes successfully on `main`:
1. Builds Linux (x86_64, aarch64) and Windows (x86_64) release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with versioned tag

### Running CI Checks Locally

Run the common local checks before pushing. CI remains authoritative and also
runs the MSRV, dependency audit and policy, platform integration, application,
hook, and wrapper jobs when the changed paths require them:

```bash
# Formatting
cargo fmt --check --manifest-path cli/Cargo.toml

# Linting
cargo clippy --profile ci --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo clippy --profile ci --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings

# Tests
cargo test --profile ci --manifest-path cli/Cargo.toml

# CI-profile build
cargo build --profile ci --manifest-path cli/Cargo.toml

# Configuration drift checks
cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift

# Configuration validation via a locally built binary
./dotfiles.sh --build -p base test

# Integration: dry-run per profile
./dotfiles.sh --build install -p base -d
./dotfiles.sh --build install -p desktop -d

# Shell wrapper lint
shellcheck --severity=warning --shell=sh --exclude=SC1090,SC1091,SC3043,SC2154 --enable=avoid-nullary-conditions dotfiles.sh install.sh

# Workflow helper lint / PowerShell analysis
export DIR="$(pwd)"
cd .github/workflows/scripts/linux
sh test-static-analysis.sh test_shellcheck
sh test-static-analysis.sh test_psscriptanalyzer
```

Pure documentation-only or `.agents/` documentation-only changes should now run
only the classifier plus the final CI gate, while workflow edits and other
uncategorized changes still force the full CI workflow for release confidence.

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
   cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift
   ./dotfiles.sh --build -p base test
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
