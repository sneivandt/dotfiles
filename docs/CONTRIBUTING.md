# Contributing to Dotfiles

Thank you for your interest in contributing! This document provides guidelines for contributing to this dotfiles project.

## Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/dotfiles.git
   cd dotfiles
   ```
3. Create a feature branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Workflow

### Before Making Changes

1. Review the project guidelines in `.github/copilot-instructions.md`
2. Understand the profile system (base, arch, arch-desktop, desktop, windows)
3. Test existing functionality:
   ```bash
   ./dotfiles.sh -T
   ```

### Making Changes

#### Adding Configuration Files

1. **Symlinks**: Add files to `symlinks/` directory, then add entries to `conf/symlinks.ini`:
   ```ini
   [base]
   config/mynewconfig
   ```

2. **Packages**: Add to appropriate section in `conf/packages.ini`:
   ```ini
   [arch]
   my-new-package
   ```

3. **Systemd Units**: Add to `conf/units.ini`:
   ```ini
   [arch,desktop]
   my-service.service
   ```

4. **File Categorization**: If the file should be excluded in certain profiles, add to `conf/manifest.ini`:
   ```ini
   [desktop]
   symlinks/config/mynewconfig
   ```

#### Creating New Profiles

Define in `conf/profiles.ini`:
```ini
[my-profile]
include=
exclude=windows,desktop
```

### Shell Scripting Guidelines

- **POSIX Compliance**: Use `#!/bin/sh` unless Bash features are absolutely required
- **Error Handling**: Always include:
  ```sh
  set -o errexit
  set -o nounset
  ```
- **Conditional Style**: Use compact format:
  ```sh
  if [ condition ]; then
    # code
  fi
  ```
- **Logging**: Use existing helpers instead of echo:
  - `log_stage "Stage Name"` - Stage headers
  - `log_verbose "Message"` - Verbose details
  - `log_error "Error"` - Error messages
  - `log_dry_run "Would <action>"` - Dry-run actions
- **Quotes**: Always quote variable expansions: `"$var"`
- **Task Functions**: Wrap in subshells for isolation:
  ```sh
  my_task() {(
    # task body
  )}
  ```
- **Idempotency**: Always check if action is needed before taking it
- **No Trailing Whitespace**: Remove all trailing whitespace from files

### PowerShell Guidelines

- **Function Names**: Use Verb-Noun convention (e.g., `Install-Symlinks`)
- **Module Exports**: Use `Export-ModuleMember` to control exports
- **Comment-Based Help**: Document functions with:
  ```powershell
  <#
  .SYNOPSIS
  Brief description
  .DESCRIPTION
  Detailed description
  #>
  ```
- **Error Handling**: Fail gracefully, check prerequisites
- **Logging Conventions**:
  - Stage headers: `Write-Output ":: Stage Name"`
  - Dry-run: `Write-Output "DRY-RUN: Would <action>"`
  - Verbose: `Write-Verbose "<message>"`
- **Idempotency**: Check state before modifications
- **Dry-Run Support**: All functions should support `-DryRun` switch
- **No Trailing Whitespace**: Remove all trailing whitespace from files
  - This applies to all file types and is a project-wide requirement
  - Trailing whitespace causes unnecessary git diffs and is poor coding hygiene
  - Configure your editor to automatically remove trailing whitespace on save

### INI Configuration Format

All `conf/*.ini` files use standard INI format:

```ini
# Comments start with #
[section-name]
entry-one
entry-two
```

**Important distinctions**:
- **Profile names** in `profiles.ini`: Use hyphens (e.g., `[arch-desktop]`)
- **Section names** in other INI files: Use comma-separated categories (e.g., `[arch,desktop]`)
  - Comma-separated means ALL categories must be active (logical AND)
- **Exception**: `registry.ini` uses `key = value` format

## Testing

### Required Before Submitting

1. **Static Analysis**:
   ```bash
   ./dotfiles.sh -T
   ```
   This runs:
   - Configuration validation (INI file syntax)
   - shellcheck (shell scripts)
   - PSScriptAnalyzer (PowerShell scripts)

2. **Dry-Run Testing**:
   ```bash
   ./dotfiles.sh -I --profile arch-desktop --dry-run
   ```
   Verify the script detects your changes correctly

3. **Verbose Testing**:
   ```bash
   ./dotfiles.sh -I --profile arch-desktop -v
   ```
   Check for proper logging output

4. **Profile-Specific Testing**:
   Test with all relevant profiles:
   - `base` - Minimal
   - `arch` - Arch Linux headless
   - `arch-desktop` - Arch Linux desktop
   - `desktop` - Generic desktop
   - `windows` - Windows (if applicable)

### CI Testing

GitHub Actions automatically runs tests on pull requests:
- Static analysis (shellcheck, PSScriptAnalyzer)
- Configuration validation
- Profile installations (dry-run) for all profiles
- Cross-platform tests (Linux Ubuntu and Windows)
- Docker image build

## Commit Guidelines

### Commit Messages

Follow conventional commit format:

```
type(scope): brief description

Optional longer description explaining the change.

Fixes #issue_number
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, no logic change)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

**Examples**:
```
feat(symlinks): add neovim configuration
fix(tasks): correct symlink permission check
docs(readme): update installation instructions
chore(ci): update shellcheck version
```

### Commits

- Keep commits atomic (one logical change per commit)
- Write clear, descriptive commit messages
- Reference issues when applicable

## Pull Request Process

1. **Before Submitting**:
   - Ensure all tests pass (`./dotfiles.sh -T`)
   - Test with `--dry-run` mode
   - Update documentation if needed
   - Remove trailing whitespace from all files
   - Verify changes with verbose mode

2. **PR Description**:
   - Describe what changes you made and why
   - Reference related issues
   - Include testing notes (which profiles tested)
   - Note any breaking changes

3. **Checklist** (automatically provided by PR template):
   - [ ] Ran `./dotfiles.sh -T` successfully
   - [ ] Tested with `--dry-run` mode
   - [ ] Tested with verbose mode (`-v`)
   - [ ] No trailing whitespace
   - [ ] Updated documentation

4. **Review Process**:
   - Address feedback from maintainers
   - Keep PR focused on single feature/fix
   - Rebase if requested to maintain clean history

## Code Style

### Shell Scripts

- Use 2 spaces for indentation
- Follow POSIX shell conventions
- Use helper functions from `src/linux/utils.sh`:
  - `read_ini_section` for INI parsing
  - `should_include_profile_tag` for profile filtering
  - `is_program_installed` for tool checks
- Document complex logic with comments

### PowerShell

- Use 4 spaces for indentation (PowerShell convention)
- Follow PowerShell best practices
- Use helper functions from `src/windows/Profile.psm1`:
  - `Read-IniSection` for INI parsing
  - `Test-ShouldIncludeSection` for profile filtering

### Configuration Files

- Use consistent section naming
- Comment complex or non-obvious entries
- Keep sections alphabetically sorted when practical
- One entry per line

## Documentation Updates

When adding features or changing behavior:

1. Update main [README.md](../README.md) if it affects usage
2. Update [WINDOWS.md](WINDOWS.md) for Windows-specific changes
3. Update [CONFIGURATION.md](CONFIGURATION.md) for configuration changes
4. Add examples to help users understand the feature

## Questions or Help

- Open an issue for questions
- Check existing issues and PRs for similar topics
- Review project documentation thoroughly first

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
