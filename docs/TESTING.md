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
./dotfiles.sh --build install -p arch-desktop -d
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

# Test arch-desktop profile
./dotfiles.sh --build install -p arch-desktop -d

# Test windows profile (on Windows)
.\dotfiles.ps1 -Build install -p windows -d
```

## Continuous Integration

### GitHub Actions CI (`.github/workflows/ci.yml`)

Runs automatically on pull requests with the following jobs:

| Job | Command | Purpose |
|---|---|---|
| `rust-fmt` | `cargo fmt --check` | Code formatting |
| `rust-clippy` | `cargo clippy -- -D warnings` | Lint checks |
| `rust-test` | `cargo test` | Unit and integration tests |
| `build-linux` | `cargo build --release` | Linux binary build + smoke test |
| `build-windows` | `cargo build --release` | Windows binary build + smoke test |
| `script-lint` | `shellcheck dotfiles.sh install.sh` | Shell wrapper linting |
| `integration-linux` | Dry-run install | Per-profile integration (base, desktop) |
| `integration-windows` | Dry-run install | Windows profile integration |
| `test-docker` | `docker build` | Docker image build + smoke test |
| `test-git-hooks` | Hook test script | Pre-commit hook validation |

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
cargo clippy --manifest-path cli/Cargo.toml -- -D warnings

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
   cargo clippy --manifest-path cli/Cargo.toml -- -D warnings
   ```

3. **Test with dry-run:**
   ```bash
   ./dotfiles.sh --build install -p base -d
   ```

4. **Test affected profiles:**
   - If modifying base configuration, test `base` profile
   - If modifying Arch-specific items, test `arch` and `arch-desktop` profiles
   - If modifying Windows items, test `windows` profile
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
