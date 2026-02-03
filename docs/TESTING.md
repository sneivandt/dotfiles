# Testing

This document describes the testing infrastructure and procedures for the dotfiles project.

## Test Suite

The project includes automated tests to validate configuration, scripts, and profile installations.

### Running Tests

Run all tests using the test flag:

```bash
./dotfiles.sh -T
# or
./dotfiles.sh --test
```

### Test Components

The test suite includes three main components:

#### 1. Configuration Validation (`test_config_validation`)
- Validates INI file syntax and structure
- Checks all configuration files in `conf/`
- Ensures proper section formatting
- Verifies profile definitions are valid

#### 2. Shell Script Linting (`test_shellcheck`)
- Runs shellcheck on all `.sh` files
- Checks POSIX compliance
- Identifies common shell scripting errors
- Validates variable usage and quoting

#### 3. PowerShell Script Analysis (`test_psscriptanalyzer`)
- Runs PSScriptAnalyzer on all `.ps1` and `.psm1` files
- Checks PowerShell best practices
- Identifies potential issues in Windows scripts

## Manual Testing

### Dry-Run Mode

Preview changes without making modifications:

```bash
./dotfiles.sh -I --dry-run
# Dry-run automatically enables verbose mode
```

All system modifications will be logged with `DRY-RUN:` prefix without being executed.

### Verbose Mode

Enable detailed logging for debugging:

```bash
./dotfiles.sh -I -v
# or
./dotfiles.sh -I --verbose
```

Shows detailed operation logs including skipped items and reasons.

### Profile Testing

Test different profiles to ensure sparse checkout and configuration work correctly:

```bash
# Test base profile
./dotfiles.sh -I --profile base --dry-run

# Test arch profile
./dotfiles.sh -I --profile arch --dry-run

# Test arch-desktop profile
./dotfiles.sh -I --profile arch-desktop --dry-run

# Test windows profile (on Windows)
./dotfiles.ps1 -Install -Profile windows -DryRun
```

## Idempotency Testing

All scripts are designed to be idempotent. The project includes both automated and manual idempotency tests.

### Automated Idempotency Tests

Idempotency tests are run automatically in CI. They validate:
- **`test_idempotency_symlinks`** - Verifies symlink installation is idempotent (runs in test-applications job)
- **Profile idempotency** - CI runs each profile installation twice:
  - `base` profile
  - `arch` profile (with --skip-os-detection)
  - `arch-desktop` profile (with --skip-os-detection)
  - `windows` profile (on Windows runner)

**Note:** Idempotency and application tests require actual installations and are run in CI jobs via `.github/workflows/scripts/linux/test-idempotency.sh` and `.github/workflows/scripts/linux/test-applications.sh`. They are not included in `./dotfiles.sh -T` which focuses on static analysis and configuration validation.

### Manual Idempotency Testing

Test by running installation multiple times:

```bash
# First run
./dotfiles.sh -I --profile arch-desktop

# Second run - should complete without errors or unnecessary changes
./dotfiles.sh -I --profile arch-desktop
```

Expected behavior:
- No errors on repeated runs
- Operations log as "Skipping: already correct" or similar
- No duplicate installations or modifications
- System state remains consistent

## Continuous Integration

### GitHub Actions Workflows

The project uses GitHub Actions for automated testing:

#### CI Workflow (`.github/workflows/ci.yml`)
Runs automatically on pull requests and pushes to validate:
- Static analysis (shellcheck and PSScriptAnalyzer)
- Configuration file validation
- Profile installations with dry-run tests
- Idempotency tests (runs installation twice for each profile)
- Cross-platform compatibility (Linux Ubuntu and Windows runners)
- Docker image build

#### Docker Image Workflow (`.github/workflows/docker-image.yml`)
Publishes Docker image to Docker Hub on pushes to master branch.

### Running CI Tests Locally

Replicate CI validation locally:

```bash
# Run static analysis and configuration validation tests
./dotfiles.sh -T

# Test each profile with dry-run
./dotfiles.sh -I --profile base --dry-run
./dotfiles.sh -I --profile arch --dry-run
./dotfiles.sh -I --profile arch-desktop --dry-run

# On Windows
./dotfiles.ps1 -DryRun
```

## Test Files

### Tests Run by `dotfiles.sh -T`
- `test/linux/test-config.sh` - Configuration validation tests
- `test/linux/test-static-analysis.sh` - Shell script linting (shellcheck, PSScriptAnalyzer)

### CI-Only Tests (require actual installations)
- `.github/workflows/scripts/linux/test-applications.sh` - Application tests (vim, nvim, zsh)
- `.github/workflows/scripts/linux/test-idempotency.sh` - Idempotency tests for installers
- `.github/workflows/scripts/windows/Test-Configuration.ps1` - Windows configuration tests
- `.github/workflows/scripts/windows/Test-Modules.ps1` - Windows module tests
- `test/windows/Test.psm1` - PowerShell test module entry point
- `test/windows/Test-StaticAnalysis.psm1` - PSScriptAnalyzer tests (also used by `dotfiles.sh -T`)

## Best Practices

When contributing changes:

1. **Run tests before committing:**
   ```bash
   ./dotfiles.sh -T
   ```

2. **Test with dry-run:**
   ```bash
   ./dotfiles.sh -I --dry-run
   ```

3. **Test affected profiles:**
   - If modifying base configuration, test `base` profile
   - If modifying Arch-specific items, test `arch` and `arch-desktop` profiles
   - If modifying Windows items, test `windows` profile

4. **Verify idempotency:**
   - Run installation twice
   - Ensure second run completes cleanly without errors

5. **Check output:**
   - Use verbose mode to verify operations
   - Ensure appropriate logging messages
   - Verify no trailing whitespace in files

## Troubleshooting Tests

### Shellcheck Failures
- Review shellcheck output for specific issues
- Ensure POSIX compliance (no Bash-specific features in `/bin/sh` scripts)
- Add shellcheck directives only when necessary with comments explaining why

### PSScriptAnalyzer Failures
- Review PSScriptAnalyzer output
- Follow PowerShell best practices
- Ensure proper error handling and parameter validation

### Configuration Validation Failures
- Check INI file syntax
- Ensure section headers use proper format:
  - Profile names in `profiles.ini`: `[profile-name]`
  - Section names in other files: `[category]` or `[category,another]`
- Verify no trailing whitespace

### Idempotency Issues
- Check for missing existence checks before operations
- Ensure proper state validation
- Add verbose logging for skipped operations
