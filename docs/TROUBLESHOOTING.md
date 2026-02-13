# Troubleshooting

Common issues and their solutions when working with the dotfiles system.

## Installation Issues

### Profile Selection

#### Profile not saved after selection
**Symptoms**: Asked to select profile every time you run the installer.

**Solution**:
```bash
# Manually save profile
git config --local dotfiles.profile arch-desktop

# Verify it's saved
git config --local --get dotfiles.profile
```

#### Wrong profile being used
**Symptoms**: Unexpected files or packages being installed.

**Solution**:
```bash
# Check current profile
git config --local --get dotfiles.profile

# Override with explicit profile
./dotfiles.sh -I --profile <correct-profile>
```

### Sparse Checkout Issues

#### Wrong files checked out
**Symptoms**: Files that shouldn't be in your workspace are present, or expected files are missing.

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

#### Desktop files missing on Arch Linux
**Symptoms**: No desktop configuration files even though you're on Arch.

**Solution**: Use the `arch-desktop` profile, not `arch`:
```bash
./dotfiles.sh -I --profile arch-desktop
```

The `arch` profile is for headless servers and excludes desktop files.

#### Sparse checkout not working
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

### Symlink Issues

#### Symlink not created
**Symptoms**: Expected symlink doesn't exist in `$HOME`.

**Possible causes and solutions**:

1. **Source file excluded by sparse checkout**:
   ```bash
   # Check if source file exists
   ls -la symlinks/<path>

   # If missing, check sparse checkout
   git sparse-checkout list
   ```

2. **Target file/directory already exists**:
   ```bash
   # Check if regular file exists at target
   ls -la ~/.<path>

   # If it's a regular file, back it up and remove
   mv ~/.<path> ~/.<path>.backup
   ./dotfiles.sh -I
   ```

3. **Entry not in correct section**:
   ```bash
   # Check conf/symlinks.ini
   # Verify entry is in a section matching your profile
   grep -A5 "\[base\]" conf/symlinks.ini
   ```

4. **Parent directory doesn't exist**:
   ```bash
   # Symlink creation should create parents automatically
   # If not, create manually:
   mkdir -p ~/.config/
   ./dotfiles.sh -I
   ```

#### Symlink points to wrong location
**Symptoms**: Symlink exists but target is incorrect.

**Solution**:
```bash
# Remove incorrect symlink
rm ~/.<path>

# Reinstall
./dotfiles.sh -I
```

#### Permission denied creating symlink (Windows)
**Symptoms**: Error creating symlinks on Windows.

**Solution**:
- Run PowerShell as Administrator
- Or enable Developer Mode (Windows 10+):
  - Settings → Update & Security → For developers → Developer Mode

### Package Installation Issues

#### Package not installed
**Symptoms**: Package from `packages.ini` wasn't installed.

**Possible causes and solutions**:

1. **Wrong section in packages.ini**:
   ```bash
   # Verify package is in correct section
   # For arch-desktop profile, package should be in [arch] or [arch,desktop]
   grep -B2 "package-name" conf/packages.ini
   ```

2. **Profile excludes the category**:
   ```bash
   # Check profile definition
   grep -A2 "\[arch-desktop\]" conf/profiles.ini

   # Verify section isn't excluded
   ./dotfiles.sh -I -v | grep "Skipping section"
   ```

3. **Package manager not available**:
   ```bash
   # Check if pacman is installed
   which pacman

   # For AUR packages, check if paru is installed
   which paru
   ```

4. **Package name incorrect**:
   ```bash
   # Search for correct package name
   pacman -Ss <package-name>
   paru -Ss <package-name>
   ```

#### AUR package installation fails
**Symptoms**: Error installing package from AUR.

**Solution**:
```bash
# Ensure paru is installed
which paru

# Install paru if missing
./dotfiles.sh -I --profile arch-desktop

# Try installing package manually to see error
paru -S <package-name>
```

#### winget package installation fails (Windows)
**Symptoms**: Error installing package on Windows.

**Solution**:
```powershell
# Verify winget is installed
winget --version

# Search for correct package ID
winget search <package-name>

# Try installing manually
winget install <PackageId>

# Update package ID in conf/packages.ini if needed
```

### Systemd Unit Issues

#### Unit not enabled
**Symptoms**: Systemd unit from `units.ini` isn't running.

**Solution**:
```bash
# Check unit status
systemctl --user status <unit-name>

# Verify unit file exists (should be symlinked)
ls -la ~/.config/systemd/user/<unit-name>

# Manually enable and start
systemctl --user enable --now <unit-name>

# Check logs
journalctl --user -u <unit-name>
```

#### Unit fails to start
**Symptoms**: Unit enabled but fails to start.

**Solution**:
```bash
# Check unit logs
journalctl --user -u <unit-name> -n 50

# Verify dependencies are met
systemctl --user list-dependencies <unit-name>

# Check unit file syntax
systemctl --user cat <unit-name>
```

### VS Code Extension Issues

#### Extensions not installed
**Symptoms**: VS Code extensions from config not installed.

**Solution**:
```bash
# Verify code CLI is available
which code
code --version

# Check if extension exists
code --list-extensions | grep <extension-id>

# Install manually
code --install-extension <extension-id>

# Verify extension ID in conf/vscode-extensions.ini
cat conf/vscode-extensions.ini
```

#### Extension installation hangs
**Symptoms**: Installation process hangs during extension installation.

**Solution**:
- Press Ctrl+C to cancel
- Try installing extensions manually
- Check VS Code marketplace availability
- Temporarily comment out extensions in `conf/vscode-extensions.ini` to skip them

### GitHub Copilot Skills Issues

#### Skills not downloaded
**Symptoms**: GitHub Copilot CLI skills from config not downloaded.

**Solution**:
```bash
# Verify gh CLI is available
which gh
gh --version

# Verify Copilot extension is installed
gh extension list | grep copilot

# Check skills directory
ls -la ~/.copilot/skills/

# Verify skill URLs in conf/copilot-skills.ini
cat conf/copilot-skills.ini
```

#### Skill download fails
**Symptoms**: Error messages during skill download or skills directory empty.

**Possible causes and solutions**:

1. **Invalid GitHub URL format**:
   ```bash
   # Verify URL format
   # Correct: https://github.com/owner/repo/blob/branch/path/to/folder
   # Incorrect: https://github.com/owner/repo/tree/branch/path/to/folder
   ```

2. **Network connectivity issues**:
   ```bash
   # Test GitHub API access
   curl -I https://api.github.com
   
   # Try downloading skill manually
   # Visit the URL in a browser to verify it exists
   ```

3. **GitHub rate limiting**:
   - Wait a few minutes and try again
   - Authenticate with GitHub: `gh auth login`

4. **Permissions on skills directory**:
   ```bash
   # Check directory permissions
   ls -ld ~/.copilot/skills/
   
   # Fix if needed
   chmod 755 ~/.copilot/skills/
   ```

## Permission Issues

### Linux

#### Cannot install packages
**Symptoms**: Permission denied when installing packages.

**Solution**:
```bash
# Ensure sudo is configured
sudo -v

# Check if user is in required groups
groups

# For Arch Linux, user should be in 'wheel' group
sudo usermod -aG wheel $USER

# Re-login for group changes to take effect
```

#### Cannot enable systemd units
**Symptoms**: Permission denied when enabling units.

**Solution**:
```bash
# Use --user flag (script should do this automatically)
systemctl --user enable <unit>

# Verify systemd user instance is running
systemctl --user status
```

### Windows

#### Script requires elevation
**Symptoms**: Error messages about requiring administrator privileges.

**Solution**:
- Right-click PowerShell
- Select "Run as Administrator"
- Re-run script

#### Cannot modify registry
**Symptoms**: Access denied when setting registry values.

**Solution**:
- Run PowerShell as Administrator
- Verify registry path is under HKCU (not HKLM)
- Check for policies preventing registry modification

## Git Issues

### Cannot pull updates
**Symptoms**: Git errors when pulling repository updates.

**Solution**:
```bash
# Stash local changes
git stash

# Pull updates
git pull

# Reapply changes
git stash pop

# Or let Windows script handle it automatically
Install-Dotfiles  # Handles stashing automatically
```

### Merge conflicts
**Symptoms**: Conflicts when pulling updates.

**Solution**:
```bash
# Check conflict status
git status

# Resolve conflicts manually
# Edit conflicted files
git add <resolved-files>
git commit

# Or abort merge and start over
git merge --abort
git pull
```

### Symlink errors (Windows)
**Symptoms**: `error: unable to create symlink: Permission denied` during git operations.

**Solution**:
```powershell
# Configure git to treat symlinks as text files
git config core.symlinks false

# Or let script configure it automatically
.\dotfiles.ps1
```

## Test Failures

### Shellcheck failures
**Symptoms**: `./dotfiles.sh -T` reports shellcheck errors.

**Solution**:
```bash
# Run shellcheck directly to see details
shellcheck src/linux/*.sh

# Fix issues in reported files
# Common issues:
# - Missing quotes around variables
# - Using non-POSIX features
# - Undefined variables
```

### PSScriptAnalyzer failures
**Symptoms**: Test reports PowerShell script issues.

**Solution**:
```powershell
# Run PSScriptAnalyzer directly
Import-Module PSScriptAnalyzer
Invoke-ScriptAnalyzer -Path src/windows/ -Recurse

# Fix reported issues
# Common issues:
# - Missing parameter validation
# - Incorrect verb usage
# - Missing help comments
```

### Configuration validation failures
**Symptoms**: Test reports invalid configuration files.

**Solution**:
```bash
# Check INI file syntax
# Ensure section headers use []
# Ensure no trailing whitespace
# Verify file references exist

# Run verbose test mode
./dotfiles.sh -T -v
```

## Profile-Specific Issues

### Base Profile

#### Minimal setup but need more
**Solution**: Switch to a more complete profile:
```bash
./dotfiles.sh -I --profile arch-desktop
```

### Arch Profile

#### Desktop files missing
**Solution**: Use `arch-desktop` instead:
```bash
./dotfiles.sh -I --profile arch-desktop
```

#### Not on Arch but want to test
**Solution**: Use `--skip-os-detection` (for testing only):
```bash
./dotfiles.sh -I --profile arch --skip-os-detection --dry-run
```

### Desktop Profile

#### Missing OS-specific packages
**Solution**: Desktop profile intentionally excludes OS-specific packages. Use OS-specific profile:
```bash
# For Arch Linux
./dotfiles.sh -I --profile arch-desktop
```

### Windows Profile

#### Cannot select profile
**Explanation**: Windows always uses the "windows" profile. Profile selection is not available on Windows.

## Dry-Run Mode Issues

### Counters show zero
**Symptoms**: Dry-run summary shows zero operations.

**Possible causes**:
1. All operations already complete (idempotency working correctly)
2. Profile doesn't match any configuration sections
3. All items excluded by profile

**Solution**:
```bash
# Run with verbose mode to see skip reasons
./dotfiles.sh -I --dry-run -v
```

## Verbose Mode Issues

### Too much output
**Symptoms**: Verbose mode shows overwhelming amount of information.

**Solution**:
- Redirect to file: `./dotfiles.sh -I -v 2>&1 | tee install.log`
- Use dry-run first to estimate scope: `./dotfiles.sh -I --dry-run`
- Check log file instead: `cat ~/.cache/dotfiles/install.log`

## Docker Issues

### Container won't start
**Symptoms**: Docker run fails.

**Solution**:
```bash
# Pull latest image
docker pull sneivandt/dotfiles

# Build locally
docker buildx build -t dotfiles:local .

# Check logs
docker logs <container-id>
```

### Container missing files
**Symptoms**: Expected files not in container.

**Solution**:
- Check Dockerfile for correct profile
- Verify sparse checkout in Dockerfile
- Rebuild image

## Getting Help

If you're still experiencing issues:

1. **Check the log file**:
   - Linux: `~/.cache/dotfiles/install.log`
   - Windows: `%LOCALAPPDATA%\dotfiles\install.log`

2. **Run with verbose mode**:
   ```bash
   ./dotfiles.sh -I -v
   ```

3. **Run tests**:
   ```bash
   ./dotfiles.sh -T -v
   ```

4. **Check existing issues**:
   - Visit the GitHub repository
   - Search for similar issues
   - Check closed issues for solutions

5. **Open a new issue**:
   - Describe the problem
   - Include error messages
   - Share relevant log excerpts
   - Specify your OS and profile
   - Mention steps to reproduce

## See Also

- [Usage Guide](USAGE.md) - Detailed usage instructions
- [Configuration Reference](CONFIGURATION.md) - Configuration file details
- [Profile System](PROFILES.md) - Understanding profiles
- [Testing Documentation](TESTING.md) - Running tests
