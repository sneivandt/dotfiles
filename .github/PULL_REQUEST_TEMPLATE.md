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
<!-- Check all profiles/categories that are affected by these changes -->
- [ ] `base` (core shell configuration)
- [ ] `desktop` (desktop tools and GUI configuration)
- [ ] Platform-specific: `arch`, `windows`, `linux` (auto-detected categories)
- [ ] Profile-agnostic (utilities, CI, documentation)

## Testing Performed
<!-- Describe the testing you've done -->
- [ ] Ran `./dotfiles.sh test` (static analysis and validation)
- [ ] Tested installation with affected profile(s)
- [ ] Verified idempotency (re-running doesn't cause errors)
- [ ] Tested in dry-run mode (`./dotfiles.sh install -d`)
- [ ] Tested with verbose mode (`./dotfiles.sh install -v`)

**Test environment:**
- OS: <!-- e.g., Arch Linux, Ubuntu, Windows 11 -->
- Profile tested: <!-- e.g., desktop -->

## Configuration Changes
<!-- If you've modified INI files, list the changes -->
- [ ] Updated `conf/packages.ini`
- [ ] Updated `conf/symlinks.ini`
- [ ] Updated `conf/systemd-units.ini`
- [ ] Updated `conf/manifest.ini`
- [ ] Updated `conf/profiles.ini`
- [ ] Other: <!-- specify -->

## Documentation
- [ ] Updated README.md if needed
- [ ] Updated WINDOWS.md if needed
- [ ] Updated conf/README.md if needed
- [ ] Code comments added/updated for complex logic

## Checklist
- [ ] Rust code passes `cargo fmt --check` and `cargo clippy -- -D warnings`
- [ ] Shell wrappers follow POSIX sh compatibility
- [ ] All new tasks are idempotent
- [ ] Added items to INI config files rather than hardcoding
- [ ] CI checks pass (automatically verified)

## Additional Notes
<!-- Any additional context, concerns, or discussion points -->
