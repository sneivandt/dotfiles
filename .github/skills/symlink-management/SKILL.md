---
name: symlink-management
description: >
  Detailed symlink conventions and management for the dotfiles project.
  Use when creating, modifying, or troubleshooting symlinks.
metadata:
  author: sneivandt
  version: "1.0"
---

# Symlink Management

This skill provides detailed guidance on symlink conventions and management in the dotfiles project.

## Overview

The dotfiles project uses symlinks to connect configuration files from the repository to their target locations. This approach:
- Keeps configuration under version control
- Allows easy updates (edit in repo, changes apply immediately)
- Supports profile-based filtering via sparse checkout
- Works cross-platform (Linux and Windows)

## Symlink Configuration

### Configuration File
**Location**: `conf/symlinks.ini`

**Format**: INI sections organized by profile categories
```ini
[base]
config/nvim
config/git/config

[arch,desktop]
config/xmonad
xinitrc

[windows]
AppData/Roaming/Code/User/settings.json
config/git/config
```

### Source Files
**Location**: `symlinks/` directory at repository root

**Convention**: Files stored WITHOUT leading dot
```
symlinks/
├── config/
│   ├── nvim/
│   ├── git/
│   └── xmonad/
├── xinitrc
└── AppData/
    └── Roaming/
        └── Code/
```

## Platform-Specific Behavior

### Linux Symlinks

#### Target Location
All targets are relative to `$HOME`, with dot prepended automatically:

**Configuration**:
```ini
[base]
config/nvim
```

**Creates**:
- Source: `<repo>/symlinks/config/nvim`
- Target: `~/.config/nvim` (dot prepended)

#### Additional Examples
```ini
[base]
config/git/config           # → ~/.config/git/config
xinitrc                     # → ~/.xinitrc
ssh/config                  # → ~/.ssh/config
```

### Windows Symlinks

#### Target Location
Targets are relative to `%USERPROFILE%` with smart dot-prefixing:

**Well-known Windows folders**: NO dot prefix
- AppData
- Documents
- Desktop
- Downloads
- Music, Videos, Pictures

**Unix-style paths**: YES dot prefix
- config
- ssh
- local

#### Examples

**Well-known folders** (no dot):
```ini
[windows]
AppData/Roaming/Code/User/settings.json
# → %USERPROFILE%\AppData\Roaming\Code\User\settings.json

Documents/WindowsPowerShell/profile.ps1
# → %USERPROFILE%\Documents\WindowsPowerShell\profile.ps1
```

**Unix-style paths** (with dot):
```ini
[windows]
config/git/config
# → %USERPROFILE%\.config\git\config

ssh/config
# → %USERPROFILE%\.ssh\config
```

#### Implementation
The smart prefixing logic is in `src/windows/Symlinks.psm1`:
```powershell
$wellKnownFolders = @(
  'AppData', 'Documents', 'Desktop', 'Downloads',
  'Music', 'Videos', 'Pictures', 'Favorites'
)

$firstPart = $relativePath.Split([IO.Path]::DirectorySeparatorChar)[0]
if ($wellKnownFolders -contains $firstPart) {
  # No dot prefix
  $target = Join-Path $env:USERPROFILE $relativePath
} else {
  # Add dot prefix
  $target = Join-Path $env:USERPROFILE ".$relativePath"
}
```

## Adding New Symlinks

### Step-by-Step Process

#### 1. Create Source File
```bash
# Without leading dot
mkdir -p symlinks/config/myapp
echo "config content" > symlinks/config/myapp/config
```

#### 2. Add to symlinks.ini
Add entry under appropriate profile section:
```ini
[base]
config/myapp/config
```

#### 3. Add to manifest.ini (if profile-specific)
If the file should only be checked out for certain profiles:
```ini
[desktop]
symlinks/config/myapp
```

This excludes the file when desktop is excluded.

#### 4. Test
```bash
# Linux
./dotfiles.sh -I --dry-run

# Windows
./dotfiles.ps1 -Install -DryRun
```

### Decision: Which Profile Section?

**`[base]`**: Core files needed everywhere
- Shell configuration
- Git config
- SSH config

**`[arch]` or `[arch,desktop]`**: Arch Linux specific
- Arch-specific configs
- Desktop environment files

**`[desktop]`**: Generic desktop files
- GUI application configs
- Desktop-independent files

**`[windows]`**: Windows specific
- PowerShell profiles
- Windows app configs
- Registry-related files

## Symlink Installation Process

### Linux Implementation
**Module**: `src/linux/tasks.sh` - `install_symlinks()`

**Process**:
1. Read all sections from `conf/symlinks.ini`
2. Filter sections by active profile
3. For each entry in matching sections:
   - Determine source: `$DIR/symlinks/<entry>`
   - Determine target: `$HOME/.<entry>` (dot prepended)
   - Create parent directories if needed
   - Check if symlink already correct (idempotency)
   - Remove existing file/directory if needed
   - Create symlink

### Windows Implementation
**Module**: `src/windows/Symlinks.psm1` - `Install-Symlinks`

**Process**:
1. Read sections from `conf/symlinks.ini`
2. Filter by excluded categories
3. For each entry:
   - Determine source: `<repo>\symlinks\<entry>`
   - Determine target with smart dot-prefixing
   - Create parent directories
   - Check if symlink already correct
   - Remove existing item if needed
   - Create symlink (requires admin on Windows)

## Idempotency

### Checking Existing Symlinks

**Linux**:
```sh
if [ -L "$target" ] && [ "$(readlink "$target")" = "$source" ]; then
  log_verbose "Skipping: already correct"
  return
fi
```

**Windows**:
```powershell
if ((Get-Item $target -ErrorAction SilentlyContinue).Target -eq $source) {
  Write-Verbose "Skipping: already correct"
  continue
}
```

### Handling Conflicts

**If target exists and is NOT a symlink**:
- Remove existing file/directory
- Create symlink
- No backup created (design decision)

**If target is a symlink but wrong**:
- Remove existing symlink
- Create correct symlink

## Directory vs File Symlinks

### Directory Symlinks
```ini
[base]
config/nvim
# Links entire directory: ~/.config/nvim → <repo>/symlinks/config/nvim
```

Benefits:
- Simpler management
- All files in directory versioned
- Easier to add/remove files

### File Symlinks
```ini
[base]
config/git/config
# Links single file: ~/.config/git/config → <repo>/symlinks/config/git/config
```

Benefits:
- Selective file management
- Mix versioned and local files
- Finer control

Choose based on use case.

## Special Cases

### Nested Symlinks
Avoid creating symlinks inside symlinked directories:
```ini
# Good - link directory
[base]
config/nvim

# Bad - link file inside already-linked directory
[base]
config/nvim
config/nvim/init.vim  # Redundant
```

### Sparse Checkout Interaction
Files in `symlinks/` are filtered by git sparse checkout based on `conf/manifest.ini`:

```ini
# manifest.ini
[desktop]
symlinks/config/xmonad

# profiles.ini
[base]
exclude=desktop

# Result: base profile won't check out symlinks/config/xmonad
```

The symlink installation will skip entries where source files don't exist (filtered by sparse checkout).

### Cross-Platform Symlinks

Some files work on both platforms:
```ini
[base]
config/git/config

[windows]
config/git/config
```

The same source file creates symlinks on both:
- Linux: `~/.config/git/config`
- Windows: `%USERPROFILE%\.config\git\config`

## Troubleshooting

### Symlink Not Created

**Check 1**: Does source file exist?
```bash
ls -la symlinks/config/myapp
```

**Check 2**: Is source filtered by sparse checkout?
```bash
git ls-files symlinks/config/myapp
```

**Check 3**: Is section included in profile?
```bash
./dotfiles.sh -I --profile base -v | grep myapp
```

### Wrong Target Location

**Check**: Verify entry in symlinks.ini doesn't have leading dot:
```ini
# Wrong
[base]
.config/nvim

# Correct
[base]
config/nvim
```

### Permission Denied (Windows)

Windows requires administrator privileges for symlink creation:
```powershell
# Run as administrator
.\dotfiles.ps1 -Install
```

Or enable Developer Mode (Windows 10+).

## Rules

- Never include leading dots in symlinks.ini entries
- Store files in symlinks/ without leading dots
- Use directory symlinks when managing entire directories
- Use file symlinks for selective file management
- Check sparse checkout interaction for profile-specific files
- Test symlink creation with dry-run mode
- Ensure idempotency (re-running is safe)
- Don't backup existing files (design decision)
- Use appropriate profile sections for organization
