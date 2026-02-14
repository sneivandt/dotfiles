---
name: troubleshooting-guide
description: >
  Common issues and solutions for the dotfiles installation and configuration.
  Use when diagnosing problems with installation, symlinks, packages, or system configuration.
metadata:
  author: sneivandt
  version: "1.0"
---

# Troubleshooting Guide

This skill documents common issues and their solutions when working with the dotfiles system.

## General Troubleshooting Approach

1. **Check verbose output**: Run with `-v` or `-Verbose` flag to see detailed information
2. **Use dry-run mode**: Run with `--dry-run` to preview changes without applying them
3. **Check log files**: Review persistent logs for errors
4. **Verify profile**: Ensure correct profile is selected
5. **Test in isolation**: Use Docker to test in a clean environment

### Verbose Output

```bash
# Linux
./dotfiles.sh -I -v

# Windows
.\dotfiles.ps1 -Verbose
```

### Log File Locations

```bash
# Linux
~/.cache/dotfiles/install.log

# Windows
%LOCALAPPDATA%\dotfiles\install.log
```

## Profile Selection Issues

### Profile Not Saved After Selection

**Symptoms**: Asked to select profile every time you run the installer.

**Solution**:
```bash
# Manually save profile
git config --local dotfiles.profile arch-desktop

# Verify it's saved
git config --local --get dotfiles.profile
```

### Wrong Profile Being Used

**Symptoms**: Unexpected files or packages being installed.

**Solution**:
```bash
# Check current profile
git config --local --get dotfiles.profile

# Override with explicit profile
./dotfiles.sh -I --profile <correct-profile>

# Or on Windows
.\dotfiles.ps1 -Profile <correct-profile>
```

### Profile Auto-Detection Override

**Symptoms**: Auto-detection selects wrong profile.

**Solution**:
- Always specify profile explicitly: `--profile <name>`
- Save profile to git config to persist choice
- Check `conf/profiles.ini` for profile definitions

## Sparse Checkout Issues

### Wrong Files Checked Out

**Symptoms**: Files that shouldn't be present, or expected files are missing.

**Solution**:
```bash
# Check sparse checkout status
git sparse-checkout list

# Check current profile
git config --local --get dotfiles.profile

# Reapply profile to fix sparse checkout
./dotfiles.sh -I --profile <your-profile>

# Force git to update working directory
git checkout HEAD -- .
```

### Desktop Files Missing on Arch Linux

**Symptoms**: No desktop configuration even on desktop system.

**Solution**: Use the `arch-desktop` profile, not `arch`:
```bash
./dotfiles.sh -I --profile arch-desktop
```

The `arch` profile is for headless servers and excludes desktop files.

### Sparse Checkout Not Working

**Symptoms**: All files present regardless of profile.

**Solution**:
```bash
# Verify sparse checkout is enabled
git sparse-checkout list

# If empty, reinitialize
git sparse-checkout init --cone

# Reapply profile
./dotfiles.sh -I --profile <your-profile>
```

## Symlink Issues

### Symlink Not Created

**Symptoms**: Expected symlink doesn't exist in `$HOME`.

**Possible causes and solutions**:

#### 1. Source file excluded by sparse checkout
```bash
# Check if source file exists
ls -la symlinks/<path>

# If missing, check sparse checkout
git sparse-checkout list

# File may be excluded - check manifest.ini
grep "<path>" conf/manifest.ini
```

#### 2. Target file/directory already exists
```bash
# Check if regular file exists at target
ls -la ~/.<path>

# If it's a regular file, back it up and remove
mv ~/.<path> ~/.<path>.backup
./dotfiles.sh -I
```

#### 3. Entry not in correct section
```bash
# Check conf/symlinks.ini
# Verify entry is in a section matching your profile
grep -A5 "\[base\]" conf/symlinks.ini
```

#### 4. Parent directory doesn't exist
```bash
# Symlink creation should create parents automatically
# If not, create manually:
mkdir -p ~/.config/
./dotfiles.sh -I
```

### Symlink Points to Wrong Location

**Symptoms**: Symlink exists but target is incorrect.

**Solution**:
```bash
# Check what symlink points to
ls -la ~/.<path>

# Remove incorrect symlink
rm ~/.<path>

# Reinstall
./dotfiles.sh -I
```

### Permission Denied Creating Symlink (Windows)

**Symptoms**: Error creating symlinks on Windows.

**Solutions**:
- Run PowerShell as Administrator
- Or enable Developer Mode (Windows 10+):
  - Settings → Update & Security → For developers → Developer Mode
- Or use directory junctions instead (automatic fallback)

## Package Installation Issues

### Package Not Installed

**Symptoms**: Package from `packages.ini` wasn't installed.

**Possible causes and solutions**:

#### 1. Wrong section in packages.ini
```bash
# Verify package is in correct section
# For arch-desktop profile, package should be in [arch] or [arch,desktop]
grep -B2 "package-name" conf/packages.ini
```

#### 2. Profile excludes the category
```bash
# Check profile definition
grep -A2 "\[arch-desktop\]" conf/profiles.ini

# Verify section isn't excluded
./dotfiles.sh -I -v | grep "Skipping section"
```

#### 3. Package manager not available
```bash
# Check if pacman is installed (Linux)
which pacman

# For AUR packages, check if paru is installed
which paru

# Check if winget is installed (Windows)
winget --version
```

#### 4. Package name incorrect
```bash
# Search for correct package name
pacman -Ss <package-name>
paru -Ss <package-name>

# Windows
winget search <package-name>
```

### AUR Package Installation Fails

**Symptoms**: Error installing package from AUR.

**Solution**:
```bash
# Ensure paru is installed
which paru

# Install paru if missing (requires git, base-devel, rust)
./dotfiles.sh -I --profile arch-desktop

# Try installing package manually to see error
paru -S <package-name>

# Check AUR page for package status
# https://aur.archlinux.org/packages/<package-name>
```

### winget Package Installation Fails (Windows)

**Symptoms**: Error installing package on Windows.

**Solution**:
```powershell
# Verify winget is installed
winget --version

# Search for correct package ID
winget search <package-name>

# Try installing manually with verbose output
winget install --id <PackageId> --verbose

# Update package ID in conf/packages.ini if needed
```

## Systemd Unit Issues

### Unit Not Enabled

**Symptoms**: Systemd unit from `units.ini` isn't running.

**Solution**:
```bash
# Check unit status
systemctl --user status <unit-name>

# Verify unit file exists (should be symlinked)
ls -la ~/.config/systemd/user/<unit-name>

# Check if unit file is in symlinks.ini
grep "<unit-name>" conf/symlinks.ini

# Check if unit is in units.ini
grep "<unit-name>" conf/units.ini

# Manually enable and start
systemctl --user enable --now <unit-name>

# Check logs
journalctl --user -u <unit-name>
```

### Unit Fails to Start

**Symptoms**: Unit enabled but fails to start.

**Solution**:
```bash
# Check unit logs
journalctl --user -u <unit-name> -n 50

# Verify dependencies are met
systemctl --user list-dependencies <unit-name>

# Check unit file syntax
systemctl --user cat <unit-name>

# Test command manually
# (extract ExecStart command from unit file and run it)
```

## VS Code Extension Issues

### Extensions Not Installed

**Symptoms**: VS Code extensions from config not installed.

**Solution**:
```bash
# Verify VS Code is installed
which code

# Check extension ID is correct
code --list-extensions | grep <extension-id>

# Install extension manually
code --install-extension <extension-id>

# Verify conf/vscode-extensions.ini format
# Should be in [extensions] section
cat conf/vscode-extensions.ini
```

### Extension Installation Hangs

**Symptoms**: Installation hangs on extension install.

**Solution**:
- Close all VS Code instances before installing
- Clear VS Code cache: `rm -rf ~/.vscode/extensions`
- Check internet connection
- Try installing extension manually first

## Git Configuration Issues

### Git Config Not Applied

**Symptoms**: Git settings from repository not used.

**Solution**:
```bash
# Check if symlink exists
ls -la ~/.config/git/config

# Check git config sources
git config --list --show-origin

# Verify sparse checkout includes git config
git sparse-checkout list | grep config/git
```

### Git Credentials Overwriting Config

**Symptoms**: Credential manager writes to wrong config file.

**Solution**:
- The dotfiles system creates `~/.gitconfig` as a write shield
- Global writes go to `~/.gitconfig`, dotfiles stay in `~/.config/git/config`
- Both files are read by git at global scope
- This is intentional behavior to protect dotfiles-tracked config

## Registry Issues (Windows)

### Registry Values Not Set

**Symptoms**: Registry values from `registry.ini` not applied.

**Solution**:
```powershell
# Check if registry.ini exists
Test-Path conf/registry.ini

# Run with verbose to see registry operations
.\dotfiles.ps1 -Verbose

# Check registry manually
Get-ItemProperty -Path "HKCU:\Software\MyApp"

# Some registry changes require admin elevation
# Run PowerShell as Administrator if needed
```

### Invalid Registry Value Type

**Symptoms**: Error about registry value type.

**Solution**:
- Check format in `registry.ini`: `ValueName=TYPE:Data`
- Valid types: REG_SZ, REG_DWORD, REG_QWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_BINARY
- Example: `Setting=REG_DWORD:1`
- Example: `Path=REG_EXPAND_SZ:%USERPROFILE%\Documents`

## Performance Issues

### Installation Takes Too Long

**Symptoms**: Installation process is very slow.

**Solutions**:
- Use `--dry-run` to preview without actual installation
- Check network connectivity (packages, extensions download)
- Check disk space
- Run verbose mode to see where time is spent
- Consider splitting large operations

### Git Operations Slow

**Symptoms**: Sparse checkout or git operations are slow.

**Solutions**:
```bash
# Check git sparse checkout status
git sparse-checkout list

# Disable cone mode if causing issues
git sparse-checkout init --no-cone

# Check repository size
du -sh .git

# Clean up git objects
git gc --aggressive
```

## CI/CD Issues

### Static Analysis Fails

**Symptoms**: `dotfiles.sh -T` fails with errors.

**Solution**:
```bash
# Run test mode to see failures
./dotfiles.sh -T

# Check shellcheck errors
shellcheck src/linux/*.sh

# Check PowerShell analysis errors
pwsh -Command "Invoke-ScriptAnalyzer -Path src/windows/*.psm1"

# Validate INI files
./dotfiles.sh -T | grep "config_validation"
```

### Docker Build Fails

**Symptoms**: `docker build` fails.

**Solution**:
```bash
# Check .dockerignore includes required files
cat .dockerignore

# Verify Dockerfile syntax
docker buildx build --check .

# Build with verbose output
docker buildx build --progress=plain -t dotfiles:test .

# Check base image is accessible
docker pull ubuntu:latest
```

## Rules for Troubleshooting

1. **Always check verbose output first**: Most issues become clear with `-v` or `-Verbose`

2. **Verify profile selection**: Many issues stem from wrong profile being used

3. **Check log files**: Persistent logs contain full history of operations

4. **Test with dry-run**: See what would happen without making changes

5. **Verify prerequisites**: Check package managers, git, and other tools are installed

6. **Check sparse checkout**: Ensure required files aren't excluded by profile

7. **Manual testing**: Try operations manually to isolate issues

8. **Use Docker for isolation**: Test in clean environment to rule out system-specific issues

9. **Check documentation**: Review relevant skills for detailed patterns

10. **Report bugs**: If issue persists, report with verbose output and logs

## Cross-References

- See the `profile-system` skill for profile selection and filtering
- See the `symlink-management` skill for symlink conventions
- See the `package-management` skill for package installation patterns
- See the `docker-usage` skill for testing in isolated environments
- See the `testing-patterns` skill for validation approaches
