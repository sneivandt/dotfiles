# Pull Request

## Description
<!-- Provide a brief description of the changes in this PR -->

## Type of Change
<!-- Check the relevant option(s) -->
- [ ] New feature (dotfiles, configuration, or script addition)
- [ ] Bug fix (corrects an issue without changing functionality)
- [ ] Configuration update (packages, symlinks, units, etc.)
- [ ] Refactoring (no functional changes)
- [ ] Documentation update
- [ ] CI/CD changes

## Affected Profiles
<!-- Check all profiles that are affected by these changes -->
- [ ] `base` (core shell configuration)
- [ ] `arch` (Arch Linux headless)
- [ ] `arch-desktop` (Arch Linux desktop)
- [ ] `windows` (Windows environment)
- [ ] Profile-agnostic (utilities, CI, documentation)

## Testing Performed
<!-- Describe the testing you've done -->
- [ ] Ran `./dotfiles.sh -T` (static analysis and validation)
- [ ] Tested installation with affected profile(s)
- [ ] Verified idempotency (re-running doesn't cause errors)
- [ ] Tested in dry-run mode (`./dotfiles.sh -I --dry-run`)
- [ ] Tested with verbose mode (`./dotfiles.sh -I -v`)

**Test environment:**
- OS: <!-- e.g., Arch Linux, Ubuntu, Windows 11 -->
- Profile tested: <!-- e.g., arch-desktop -->

## Configuration Changes
<!-- If you've modified INI files, list the changes -->
- [ ] Updated `conf/packages.ini`
- [ ] Updated `conf/symlinks.ini`
- [ ] Updated `conf/units.ini`
- [ ] Updated `conf/submodules.ini`
- [ ] Updated `conf/manifest.ini`
- [ ] Updated `conf/profiles.ini`
- [ ] Other: <!-- specify -->

## Documentation
- [ ] Updated README.md if needed
- [ ] Updated WINDOWS.md if needed
- [ ] Updated conf/README.md if needed
- [ ] Code comments added/updated for complex logic

## Checklist
- [ ] Code follows POSIX sh compatibility (for shell scripts)
- [ ] PowerShell functions follow Verb-Noun naming convention
- [ ] All new scripts are idempotent
- [ ] No trailing whitespace in any files
- [ ] Used existing logging helpers (`log_stage`, `log_verbose`, etc.)
- [ ] Added items to INI config files rather than hardcoding
- [ ] CI checks pass (automatically verified)

## Additional Notes
<!-- Any additional context, concerns, or discussion points -->
