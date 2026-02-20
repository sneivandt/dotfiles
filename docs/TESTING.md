# Testing

This document describes the testing infrastructure and procedures for the dotfiles project.

## Test Suite

The project uses Rust's built-in test framework. All tests are run with `cargo test`.

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

#### 1. Unit Tests (inline `#[cfg(test)]` modules)

Unit tests live alongside the code they test in `cli/src/`. Examples:
- `platform.rs` — Platform detection and category exclusion logic
- `cli.rs` — CLI argument parsing and command structure
- `config/ini.rs` — INI file parsing

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

#### 2. Configuration Validation

The `test` subcommand validates all configuration files at runtime:

```bash
# Via the binary directly
./dotfiles.sh test

# Or in dev mode
./dotfiles.sh --build test
```

This checks:
- INI file syntax and structure
- Section format
- Profile definitions
- File references

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
.\dotfiles.ps1 -Build install -p desktop -d
```

## Continuous Integration

### GitHub Actions CI (`.github/workflows/ci.yml`)

Runs automatically on pull requests with the following jobs:

| Job | Matrix | Purpose |
|---|---|---|
| `rust` | fmt, clippy, test | Rust formatting, linting, and unit/integration tests |
| `lint` | ShellCheck, PSScriptAnalyzer | Static analysis for shell and PowerShell scripts |
| `validate-config` | 6 config checks | INI syntax, file references, category consistency |
| `build` | Linux, Windows | Release binary build + smoke test |
| `integration` | base/Linux, desktop/Linux, base/Windows | Dry-run install and validation per profile |
| `test-applications` | git, zsh, vim, nvim | Application-specific behavior tests |
| `test-paru` | — | AUR helper bootstrap validation (Arch container) |
| `test-docker` | — | Docker image build + smoke test |
| `test-git-hooks` | — | Pre-commit sensitive data detection |
| `test-shell-wrapper-linux` | — | Linux wrapper script validation |
| `test-shell-wrapper-windows` | — | Windows wrapper script validation |

### Release Pipeline (`.github/workflows/release.yml`)

Triggers on push to `master` when `cli/` or `conf/` change:
1. Builds Linux and Windows release binaries
2. Generates SHA-256 checksums
3. Creates a GitHub Release with versioned tag

### Running CI Checks Locally

Replicate the full CI validation locally:

```bash
# Formatting
cargo fmt --check --manifest-path cli/Cargo.toml

# Linting
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings

# Tests
cargo test --manifest-path cli/Cargo.toml

# Release build
cargo build --release --manifest-path cli/Cargo.toml

# Integration: dry-run per profile
./dotfiles.sh --build install -p base -d
./dotfiles.sh --build install -p desktop -d

# Shell wrapper lint
shellcheck dotfiles.sh install.sh
```

## Best Practices

When contributing changes:

1. **Run tests before committing:**
   ```bash
   cargo test --manifest-path cli/Cargo.toml
   ```

2. **Run all lints:**
   ```bash
   cargo fmt --check --manifest-path cli/Cargo.toml
   cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
   ```

3. **Test with dry-run:**
   ```bash
   ./dotfiles.sh --build install -p base -d
   ```

4. **Test affected profiles:**
   - If modifying base configuration, test `base` profile
   - If modifying desktop items, test `desktop` profile
   - Platform categories (arch, windows) are auto-detected and tested in CI
   - See [Profile System](PROFILES.md) for profile details

5. **Verify no trailing whitespace** in all files

## See Also

- [Contributing Guide](CONTRIBUTING.md) - Development workflow
- [Architecture](ARCHITECTURE.md) - Implementation details
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Usage Guide](USAGE.md) - Testing different profiles

## Troubleshooting Tests

### Cargo Test Failures
- Review test output for specific assertions
- Run with `-- --nocapture` to see println output
- Check that `cli/Cargo.lock` is committed

### Clippy Warnings
- All warnings are treated as errors (`-D warnings`)
- Fix the warning or add a targeted `#[allow()]` with a comment explaining why

### Configuration Validation Failures
- Check INI file syntax
- Ensure section headers use proper format:
  - Profile names in `profiles.ini`: `[profile-name]`
  - Section names in other files: `[category]` or `[category,another]`
- Verify no trailing whitespace

### Integration Test Failures
- Ensure the binary builds: `cargo build --release --manifest-path cli/Cargo.toml`
- Check that `conf/` files are valid
- Run the failing profile manually with `-d` (dry-run) and `-v` (verbose)
