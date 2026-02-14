---
name: contributing-guide
description: >
  Contributing guidelines and development workflow for the dotfiles project.
  Use when contributing changes or reviewing pull requests.
metadata:
  author: sneivandt
  version: "1.0"
---

# Contributing Guide

This skill provides guidelines for contributing to the dotfiles project and following the development workflow.

## Getting Started

### 1. Fork and Clone
```bash
# Fork on GitHub, then clone your fork
git clone https://github.com/YOUR_USERNAME/dotfiles.git
cd dotfiles
```

### 2. Create Feature Branch
```bash
git checkout -b feature/your-feature-name
```

### 3. Familiarize with Documentation
Review key documentation before making changes:
- `.github/copilot-instructions.md` - Project guidelines
- `.github/skills/` - Detailed technical patterns
- `docs/ARCHITECTURE.md` - System design
- `docs/PROFILES.md` - Profile system
- `docs/CONFIGURATION.md` - Configuration reference
- `docs/TESTING.md` - Testing procedures

## Development Workflow

### Before Making Changes

#### 1. Understand the Profile System
The project uses profiles to support multiple environments:
- **base**: Minimal core shell configuration
- **arch**: Arch Linux headless
- **arch-desktop**: Arch Linux desktop
- **desktop**: Generic Linux desktop
- **windows**: Windows environment

See the `profile-system` skill for details.

#### 2. Test Current State
```bash
./dotfiles.sh -T
```

This runs:
- Configuration validation
- shellcheck on all shell scripts
- PSScriptAnalyzer on PowerShell scripts

#### 3. Review Existing Patterns
- Check `.github/skills/` for relevant patterns
- Look at similar existing code
- Follow established conventions

### Making Changes

#### Adding Configuration Items

**1. Symlinks**
Add files to `symlinks/` directory (without leading dot):
```bash
# Create the file
mkdir -p symlinks/config/myapp
echo "config content" > symlinks/config/myapp/config

# Add to conf/symlinks.ini
[base]
config/myapp/config
```

**2. Packages**
Add to appropriate profile section in `conf/packages.ini`:
```ini
[arch]
my-new-package

[arch,desktop]
desktop-only-package
```

**3. Systemd Units**
Add to `conf/units.ini`:
```ini
[arch,desktop]
my-service.service
```

**4. File Categorization**
If files should be excluded in certain profiles, add to `conf/manifest.ini`:
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

#### Shell Script Changes

Follow the patterns in `shell-patterns` skill:
- Use `#!/bin/sh` for POSIX compatibility
- Include `set -o errexit` and `set -o nounset`
- Use compact conditional style: `if [ condition ]; then`
- Quote all variable expansions: `"$var"`
- Use logging helpers: `log_stage`, `log_verbose`, `log_error`
- Wrap tasks in subshells: `my_task() {( ... )}`
- Ensure idempotency

#### PowerShell Script Changes

Follow the patterns in `powershell-patterns` skill:
- Use Verb-Noun function names
- Include comment-based help
- Support `-DryRun` switch
- Use logging conventions: `Write-Output ":: Stage"`, `Write-Verbose`
- Export only necessary functions

#### INI File Changes

Follow the format in `ini-configuration` skill:
- Use section headers: `[section-name]`
- Profile names use hyphens: `[arch-desktop]`
- Section names use commas: `[arch,desktop]`
- One entry per line
- Comments start with `#`

### Testing Changes

#### 1. Static Analysis
```bash
./dotfiles.sh -T
```

#### 2. Dry-Run Testing
```bash
# Test relevant profiles
./dotfiles.sh -I --profile base --dry-run
./dotfiles.sh -I --profile desktop --dry-run

# Windows
./dotfiles.ps1 -Install -Profile windows -DryRun
```

#### 3. Idempotency Testing
```bash
# Run twice - second run should skip everything
./dotfiles.sh -I --profile base
./dotfiles.sh -I --profile base -v
```

#### 4. Manual Verification
Test your changes work as intended:
- Check symlinks are created correctly
- Verify packages install properly
- Test with different profiles

### Code Style

#### No Trailing Whitespace
Remove all trailing whitespace:
```bash
# Many editors can auto-remove on save
# Or manually check:
git diff --check
```

#### Shell Style
```sh
# Good
if [ -f "$file" ]; then
  log_verbose "Processing $file"
fi

# Bad - no quotes, wrong style
if [ -f $file ]
then
  echo "Processing $file"
fi
```

#### PowerShell Style
```powershell
# Good
function Install-Package {
  [CmdletBinding()]
  param([string]$Name)
  Write-Output ":: Installing packages"
}

# Bad - wrong verb, no help
function Download-Package($Name) {
  echo "Installing"
}
```

## Pull Request Process

### 1. Commit Your Changes
```bash
git add .
git commit -m "Brief description of changes"
```

Use clear, descriptive commit messages:
- Good: "Add tmux configuration for base profile"
- Bad: "Update files"

### 2. Push to Your Fork
```bash
git push origin feature/your-feature-name
```

### 3. Create Pull Request
- Go to GitHub and create a pull request
- Fill out the PR template
- Describe what changed and why
- Reference any related issues

### 4. CI/CD Validation
GitHub Actions will automatically:
- Run static analysis
- Validate configurations
- Test profile installations
- Build Docker image

Fix any CI failures.

### 5. Code Review
- Address reviewer feedback
- Push additional commits as needed
- Keep discussion focused and professional

### 6. Merge
Once approved and CI passes, the PR will be merged.

## Common Contribution Scenarios

### Adding a New Package
1. Add to `conf/packages.ini` under appropriate section
2. Test with dry-run: `./dotfiles.sh -I --dry-run`
3. Verify package name is correct for the OS
4. Consider if it needs manifest entry

### Adding a New Symlink
1. Create file in `symlinks/` (without leading dot)
2. Add entry to `conf/symlinks.ini`
3. Test symlink creation
4. Add to `conf/manifest.ini` if profile-specific

### Fixing a Shell Script Bug
1. Fix the issue
2. Run shellcheck: `./dotfiles.sh -T`
3. Test the script works
4. Verify idempotency

### Adding a New Skill
1. Create `.github/skills/skill-name/SKILL.md`
2. Follow the `creating-skills` skill guidance
3. Update `.github/copilot-instructions.md`
4. Test with `./dotfiles.sh -T`

## Best Practices

### Do
- ✅ Test all changes before committing
- ✅ Follow existing patterns and conventions
- ✅ Write clear commit messages
- ✅ Keep changes focused and minimal
- ✅ Document significant changes
- ✅ Test idempotency
- ✅ Check for trailing whitespace
- ✅ Run the test suite

### Don't
- ❌ Commit sensitive data (keys, passwords)
- ❌ Break existing functionality
- ❌ Skip testing
- ❌ Mix unrelated changes
- ❌ Use platform-specific features without checking
- ❌ Hardcode paths or values
- ❌ Ignore linter warnings without justification

## Getting Help

### Resources
- **Documentation**: `docs/` directory
- **Skills**: `.github/skills/` directory
- **Examples**: Look at existing code
- **Issues**: GitHub issues for questions

### Questions
- Check documentation first
- Search existing issues
- Create a new issue with clear description
- Be specific about your environment

## Rules

- All contributions must pass CI/CD checks
- Follow the existing code style and conventions
- Test changes with dry-run mode
- Ensure idempotency
- No trailing whitespace
- Update documentation for significant changes
- Keep commits focused and well-described
- Be respectful and professional in all interactions
