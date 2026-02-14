---
name: customization-guide
description: >
  Guide for adding new configuration items to the dotfiles project.
  Use when adding symlinks, packages, systemd units, VS Code extensions, or Windows registry settings.
metadata:
  author: sneivandt
  version: "1.0"
---

# Customization Guide

This skill provides step-by-step guidance for extending the dotfiles system with new configuration items.

## Overview

The dotfiles system supports multiple types of configuration items:
- **Symlinks**: Link configuration files from repository to home directory
- **Packages**: Install system packages (Linux/Windows)
- **Systemd Units**: Enable systemd user services and timers (Linux)
- **VS Code Extensions**: Install VS Code extensions
- **Copilot Skills**: Download external GitHub Copilot CLI skills
- **Registry Settings**: Configure Windows registry values (Windows)
- **File Permissions**: Set chmod permissions on files (Linux)
- **Fonts**: Check and install font families

All configurations use INI files in the `conf/` directory and are profile-aware.

## Adding Symlinks

### Single File Symlink

1. **Create the source file** in `symlinks/` directory (without leading dot):
```bash
mkdir -p symlinks/config/mynewapp
echo "my config" > symlinks/config/mynewapp/config.yml
```

2. **Add entry to conf/symlinks.ini**:
```ini
[base]
config/mynewapp/config.yml
```
Or for profile-specific config:
```ini
[arch,desktop]
config/mynewapp/config.yml
```

3. **Optionally add to manifest.ini** (if file should be excluded in certain profiles):
```ini
[desktop]
symlinks/config/mynewapp/
```

4. **Install the symlink**:
```bash
./dotfiles.sh -I
```

The file will be symlinked: `symlinks/config/mynewapp/config.yml` → `~/.config/mynewapp/config.yml`

### Directory Symlink

For applications with multiple config files, link the entire directory:

```bash
# Create directory structure
mkdir -p symlinks/config/myapp
touch symlinks/config/myapp/config.yml
touch symlinks/config/myapp/themes.yml
```

Add to `conf/symlinks.ini`:
```ini
[base]
config/myapp
```

This links the entire directory: `symlinks/config/myapp` → `~/.config/myapp`

See the `symlink-management` skill for detailed symlink conventions.

## Adding Packages

### Linux Packages

1. **Find the correct package name**:
```bash
# Official repositories
pacman -Ss <search-term>

# AUR packages
paru -Ss <search-term>
```

2. **Edit conf/packages.ini**:
```ini
[arch]
my-package
another-package

[arch,desktop]
desktop-package

[arch,aur]
my-aur-package-bin

[arch,desktop,aur]
desktop-aur-package
```

3. **Install**:
```bash
./dotfiles.sh -I
```

### Windows Packages

1. **Find package ID**:
```powershell
winget search <package-name>
# Note the exact Package ID (e.g., Microsoft.PowerShell)
```

2. **Edit conf/packages.ini**:
```ini
[windows]
Microsoft.PowerShell
Microsoft.VisualStudioCode
Git.Git
```

3. **Install**:
```powershell
.\dotfiles.ps1
```

See the `package-management` skill for detailed package patterns.

## Adding Systemd Units

### Service Unit

1. **Create unit file in symlinks**:
```bash
mkdir -p symlinks/config/systemd/user
cat > symlinks/config/systemd/user/my-service.service << 'EOF'
[Unit]
Description=My Custom Service

[Service]
ExecStart=/usr/bin/myapp

[Install]
WantedBy=default.target
EOF
```

2. **Add unit file to conf/symlinks.ini**:
```ini
[base]
config/systemd/user/my-service.service
```

3. **Add to conf/units.ini to enable it**:
```ini
[base]
my-service.service
```

4. **Install and enable**:
```bash
./dotfiles.sh -I
# Unit is automatically symlinked and enabled
```

### Timer Unit

Timer units require both a service and timer file:

```bash
# Create service file
cat > symlinks/config/systemd/user/my-task.service << 'EOF'
[Unit]
Description=My Periodic Task

[Service]
Type=oneshot
ExecStart=/usr/bin/my-script.sh
EOF

# Create timer file
cat > symlinks/config/systemd/user/my-task.timer << 'EOF'
[Unit]
Description=Run My Task Daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
EOF
```

Add both to `conf/symlinks.ini` and the timer to `conf/units.ini`:
```ini
# symlinks.ini
[base]
config/systemd/user/my-task.service
config/systemd/user/my-task.timer

# units.ini
[base]
my-task.timer
```

## Adding VS Code Extensions

1. **Find extension ID**:
   - Open VS Code
   - Go to Extensions view
   - Click on extension
   - Copy the ID (e.g., `ms-python.python`)

2. **Add to conf/vscode-extensions.ini**:
```ini
[extensions]
ms-python.python
rust-lang.rust-analyzer
github.copilot
```

3. **Install**:
```bash
./dotfiles.sh -I
# Or on Windows:
.\dotfiles.ps1
```

**Notes**:
- The `[extensions]` section is special - not profile-based
- Extensions are installed via `code --install-extension`
- Requires VS Code to be installed

## Adding GitHub Copilot CLI Skills

1. **Find skill folder URL**:
   - Browse GitHub repositories with Copilot skills
   - Find the folder containing skill definition files
   - Copy the GitHub URL (e.g., `https://github.com/owner/repo/blob/main/skills/skill-name`)

2. **Add to conf/copilot-skills.ini**:
```ini
[base]
https://github.com/github/awesome-copilot/blob/main/skills/azure-devops-cli

[desktop]
https://github.com/example/skills/blob/main/skills/web-dev-helper
```

3. **Install**:
```bash
./dotfiles.sh -I
# Or on Windows:
.\dotfiles.ps1
```

**Notes**:
- Skills are downloaded to `~/.copilot/skills/` directory
- The entire folder (including subdirectories) is downloaded
- Requires GitHub Copilot CLI (`gh copilot`) to be functional
- Skills are profile-aware - use appropriate sections

## Adding Registry Settings (Windows)

1. **Identify registry key and value**:
   - Use Registry Editor (regedit) to find the key
   - Note the full path, value name, type, and data

2. **Add to conf/registry.ini** using registry path as section:
```ini
[HKCU\Software\MyApp]
SettingName=REG_SZ:My Value
EnableFeature=REG_DWORD:1
FilePath=REG_EXPAND_SZ:%USERPROFILE%\Documents

[HKCU\Control Panel\Desktop]
AutoColorization=REG_DWORD:0
```

**Format**: `ValueName=TYPE:Data`

**Supported Types**:
- `REG_SZ`: String value
- `REG_DWORD`: 32-bit integer (use decimal or 0x hex)
- `REG_QWORD`: 64-bit integer
- `REG_EXPAND_SZ`: Expandable string (with environment variables)
- `REG_MULTI_SZ`: Multi-string value (use `\0` as separator)
- `REG_BINARY`: Binary data (hex string)

3. **Apply settings**:
```powershell
.\dotfiles.ps1
```

**Notes**:
- Registry settings are Windows-only (no profile filtering)
- Keys are created if they don't exist
- Requires admin elevation for HKLM keys

## Adding File Permissions (Linux)

1. **Add to conf/chmod.ini**:
```ini
[base]
700 config/myapp/secrets.yml
755 bin/my-script.sh

[arch,desktop]
644 config/desktop-app/config.ini
```

**Format**: `<mode> <relative-path-under-home>`

2. **Apply permissions**:
```bash
./dotfiles.sh -I
```

**Notes**:
- Paths are relative to `$HOME` and prefixed with `.` automatically
- Permissions are applied recursively with `-R` flag
- Skips gracefully if target file doesn't exist

## Adding Fonts (Linux)

1. **Add to conf/fonts.ini**:
```ini
[base]
JetBrains Mono
FiraCode Nerd Font

[arch,desktop]
Noto Sans
```

2. **Install fonts and update cache**:
```bash
./dotfiles.sh -I
```

**Notes**:
- Font names are checked with `fc-list` command
- Only missing fonts trigger cache update
- Fonts must be installed separately (via packages or manually)

## Profile Selection Guidelines

When adding configuration items, choose the appropriate profile section:

- **`[base]`**: Essential items needed on all systems
- **`[arch]`**: Arch Linux-specific items (headless + desktop)
- **`[arch,desktop]`**: Desktop environment items (GUI apps)
- **`[desktop]`**: Cross-platform desktop items
- **`[windows]`**: Windows-specific items
- **`[extensions]`**: VS Code extensions (special case - not profile-filtered)

See the `profile-system` skill for complete profile details.

## Rules for Adding Configuration Items

1. **Use appropriate configuration files**: Each item type has a dedicated INI file in `conf/`

2. **Follow INI format**: Use section headers with comma-separated categories for profile filtering

3. **Test with dry-run first**: Run `./dotfiles.sh -I --dry-run` to preview changes before applying

4. **One item per line**: Don't combine multiple items on a single line

5. **Use relative paths**: Paths in symlinks.ini and chmod.ini are relative to home directory

6. **No leading dots in symlinks/**: Source files in `symlinks/` directory have no leading dots

7. **Document special items**: Add comments in INI files for non-obvious configurations

8. **Test across profiles**: If adding profile-specific items, test with different profiles

9. **Commit all parts**: When adding symlinks, commit both the source file and the INI entry

10. **Update manifest.ini if needed**: Add sparse checkout exclusions for profile-specific files

## Cross-References

- See the `profile-system` skill for profile filtering and sparse checkout
- See the `symlink-management` skill for detailed symlink conventions
- See the `package-management` skill for package installation patterns
- See the `ini-configuration` skill for INI file format details
