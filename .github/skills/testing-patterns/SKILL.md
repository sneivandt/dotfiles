---
name: testing-patterns
description: >
  Testing conventions and validation patterns for the dotfiles project.
  Use when creating tests, running validation, or setting up CI/CD.
metadata:
  author: sneivandt
  version: "1.0"
---

# Testing Patterns

This skill provides guidance on testing conventions and validation patterns used in the dotfiles project.

## Test Suite Overview

The project includes comprehensive testing to validate configuration, scripts, and installations:

- **Configuration Validation**: INI file syntax and structure
- **Static Analysis**: Shell and PowerShell linting
- **Idempotency Tests**: Verify repeated runs are safe
- **Profile Tests**: Validate different profile configurations

## Running Tests

### All Tests
```bash
./dotfiles.sh -T
# or
./dotfiles.sh --test
```

### Test Components

#### 1. Configuration Validation (`test_config_validation`)
Validates all configuration files in `conf/`:
- INI file syntax checking
- Section format validation
- Profile definition consistency
- Category consistency across files
- Empty section detection

Implementation: `src/linux/commands.sh`

#### 2. Shell Script Linting (`test_shellcheck`)
Runs shellcheck on all `.sh` files:
- POSIX compliance checking
- Variable usage validation
- Quoting issues detection
- Common scripting errors

Requires: `shellcheck` package

#### 3. PowerShell Script Analysis (`test_psscriptanalyzer`)
Runs PSScriptAnalyzer on all `.ps1` and `.psm1` files:
- PowerShell best practices
- Cmdlet usage validation
- Parameter validation
- Common PowerShell issues

Requires: `PSScriptAnalyzer` module (installed via `pwsh`)

## Manual Testing Modes

### Dry-Run Mode
Preview changes without making modifications:

```bash
./dotfiles.sh -I --dry-run
```

Characteristics:
- Automatically enables verbose mode
- Shows "DRY-RUN: Would <action>" messages
- No system modifications made
- Full operation logging to file

Use cases:
- Testing new configurations
- Previewing profile changes
- Verifying installation steps
- CI/CD validation

### Verbose Mode
Enable detailed logging:

```bash
./dotfiles.sh -I -v
# or
./dotfiles.sh -I --verbose
```

Shows:
- Detailed operation logs
- Skipped items with reasons
- File operations
- Configuration processing

Use for debugging and understanding behavior.

## Profile Testing

Test each profile to ensure sparse checkout and configuration work correctly:

```bash
# Linux profiles
./dotfiles.sh -I --profile base --dry-run
./dotfiles.sh -I --profile arch --dry-run --skip-os-detection
./dotfiles.sh -I --profile arch-desktop --dry-run --skip-os-detection
./dotfiles.sh -I --profile desktop --dry-run

# Windows profile
./dotfiles.ps1 -Install -Profile windows -DryRun
```

**Note**: Use `--skip-os-detection` for testing Arch-specific profiles on non-Arch systems in dry-run mode.

## Idempotency Testing

All operations must be idempotent (safe to run multiple times).

### Automated Tests
CI runs each profile installation twice to verify idempotency:
- First run: Initial installation
- Second run: Should skip all actions (already configured)

### Manual Idempotency Verification
```bash
# First run - full installation
./dotfiles.sh -I --profile base

# Second run - should skip everything
./dotfiles.sh -I --profile base -v

# Check logs for "Skipping" messages
```

### Idempotency Patterns

#### Check Before Action
```sh
# Check if symlink already correct
if [ -L "$target" ] && [ "$(readlink "$target")" = "$source" ]; then
  log_verbose "Skipping: already correct"
  return
fi
```

#### Skip Already Installed
```sh
# Check if package installed
if is_program_installed "$package"; then
  log_verbose "Skipping $package: already installed"
  continue
fi
```

#### Verify State First
```powershell
# Check registry value
if (Test-RegistryValue $key $name $value) {
  Write-Verbose "Skipping: already set"
  continue
}
```

## CI/CD Integration

### GitHub Actions Workflows

#### Main CI Workflow (`.github/workflows/ci.yml`)
Runs on every pull request:
- Static analysis (shellcheck, PSScriptAnalyzer)
- Configuration validation
- Dry-run profile tests on Ubuntu and Windows
- Docker image build test

#### Docker Image Workflow (`.github/workflows/docker-image.yml`)
Runs on pushes to master:
- Builds Docker image
- Publishes to Docker Hub
- Tags with version and latest

### Test Matrix
CI tests all profiles across platforms:

**Linux (Ubuntu runner)**:
- base profile (dry-run)
- arch profile (dry-run with --skip-os-detection)
- arch-desktop profile (dry-run with --skip-os-detection)
- desktop profile (dry-run)

**Windows (Windows runner)**:
- windows profile (dry-run)

## Writing Tests

### Test Function Pattern
Located in `src/linux/commands.sh`:

```sh
test_my_feature()
{(
  log_stage "Testing feature"

  # Setup
  test_data="value"

  # Run test
  if ! validate_something "$test_data"; then
    log_error "Test failed: validation error"
  fi

  # Cleanup if needed
  log_verbose "Test passed"
)}
```

### Adding New Tests

1. Add test function to `src/linux/commands.sh` or test scripts
2. Call from `do_test()` function
3. Use `log_error` to fail tests
4. Ensure idempotency - tests should be runnable multiple times

## Debugging Failed Tests

### ShellCheck Failures
1. Review the reported line and issue code (SC####)
2. Check shellcheck wiki: https://www.shellcheck.net/wiki/SC####
3. Fix the issue or add suppression comment if false positive:
   ```sh
   # shellcheck disable=SC2086
   ```

### PSScriptAnalyzer Failures
1. Review the rule name and message
2. Fix the issue following PowerShell best practices
3. Or suppress with:
   ```powershell
   [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSRuleName', '')]
   ```

### Configuration Validation Failures
1. Check INI file syntax
2. Verify section headers use correct format: `[section-name]`
3. Ensure categories are consistent across files
4. Check for empty sections

## Test Coverage

Current test coverage includes:
- ✅ INI configuration syntax and structure
- ✅ Shell script POSIX compliance
- ✅ PowerShell script best practices
- ✅ Profile sparse checkout functionality
- ✅ Symlink installation idempotency
- ✅ Multi-profile support
- ✅ Cross-platform compatibility (Linux + Windows)

## Rules

- All code changes must pass the test suite
- New features should include appropriate tests
- Tests must be idempotent (safe to run multiple times)
- Use dry-run mode for CI/CD validation
- Profile tests must cover all supported profiles
- Static analysis issues must be fixed or justified
- Always test idempotency manually before committing
