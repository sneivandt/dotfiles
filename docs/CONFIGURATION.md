# Configuration Files

The `conf/` directory contains all configuration files for the dotfiles management system. All files use standard INI format with section headers for organization.

## Overview

The configuration system is based on **profiles** that control:
- Which files are checked out via git sparse checkout
- Which packages, units, and other components are installed
- Which symlinks are created

Configuration files are processed automatically based on the active profile. Items defined in matching profile sections are automatically installed without requiring additional flags.

## Configuration File Format

All configuration files use standard INI format:

```ini
# Comments start with #
[section-name]
entry-one
entry-two

[another-section]
more-entries
```

**Key rules:**
- **Profile names** (in profiles.ini only) can use hyphens: `[arch-desktop]`
- **Section names** (in all other .ini files) use comma-separated categories: `[arch,desktop]`
  - This indicates the section requires ALL listed categories to be active (AND logic)
  - Example: `[arch,desktop]` is only processed when both `arch` AND `desktop` are not excluded
  - **Exception**: `manifest.ini` uses OR logic—`[arch,desktop]` means exclude if arch OR desktop is excluded
- Empty lines and comments starting with `#` are ignored
- Only sections whose required categories match the active profile are processed
- `registry.ini` uses `key = value` format with registry paths as sections (Windows-only)

## Available Profiles

Profiles are defined in `profiles.ini`:

- **`base`**: Minimal core shell configuration (cross-platform)
- **`arch`**: Arch Linux headless (includes Arch packages, excludes desktop)
- **`arch-desktop`**: Arch Linux desktop (includes desktop tools, window manager, fonts)
- **`desktop`**: Generic Linux desktop (includes desktop tools like VS Code and IntelliJ IDEA, excludes OS-specific packages)
- **`windows`**: Windows environment (PowerShell, registry settings, desktop tools like VS Code and IntelliJ IDEA)

## Configuration Files

### `profiles.ini`
**Purpose**: Defines available profiles and their include/exclude categories.

**Format**: Each profile specifies categories to include or exclude.

**Example**:
```ini
[arch-desktop]
include=arch,desktop
exclude=windows
```

**Categories**:
- `windows`: Windows-specific configuration
- `arch`: Arch Linux-specific configuration
- `desktop`: Desktop/GUI configuration (cross-platform)

---

### `manifest.ini`
**Purpose**: Maps files and directories to categories for git sparse checkout exclusion.

**Format**: Sections represent categories; entries are paths relative to repository root.

**Example**:
```ini
[desktop]
symlinks/config/xmonad/
symlinks/Xresources

[arch,desktop]
symlinks/config/dunst/
```

**Important: OR Logic for Exclusions**:
- Unlike other configuration files, manifest.ini uses **OR logic** for multi-category sections
- `[arch,desktop]` means "exclude if arch OR desktop is excluded" (not both required)
- This ensures files common to multiple categories are excluded if ANY of those categories is excluded
- Other config files use AND logic: `[arch,desktop]` means "include only if BOTH arch AND desktop are active"

**How it works**: When a profile excludes a category, files listed under sections containing that category are excluded from sparse checkout.

---

### `symlinks.ini`
**Purpose**: Defines symlinks to create in `$HOME`.

**Format**: Sections represent profiles; entries are paths relative to `$HOME` (without leading dot).

**Example**:
```ini
[base]
bashrc
config/git/config
config/nvim

[arch,desktop]
xinitrc
config/xmonad/xmonad.hs
```

**How it works**:
- Source files are located in `symlinks/<path>` at repository root
- Targets are created at `~/.<path>`
- Example: `config/nvim` → `~/.config/nvim` symlinked to `<repo>/symlinks/config/nvim`

---

### `packages.ini`
**Purpose**: Lists system packages to install via package manager.
- **Linux**: Uses `pacman` (Arch Linux) and `paru` (AUR)
- **Windows**: Uses `winget`

**Format**: Sections represent profiles; entries are package names.
- Sections with `aur` tag are handled by `paru` (e.g., `[arch,aur]`)
- Arch Linux sections without the `aur` tag (e.g., `[arch]`, `[arch,desktop]`) are handled by the standard package manager (`pacman`)
- Windows sections (e.g., `[windows]`) are handled by `winget`

**Example**:
```ini
[arch]
git
base-devel

[arch,aur]
powershell-bin

[arch,desktop]
alacritty
```

---

### `units.ini`
**Purpose**: Lists systemd user units to enable and start.

**Format**: Sections represent profiles; entries are unit filenames.

**Example**:
```ini
[base]
clean-home-tmp.timer

[desktop]
picom.service
dunst.service
```

**Note**: Unit files should exist in `symlinks/config/systemd/user/` and be symlinked before enabling.

---

### `chmod.ini`
**Purpose**: Specifies file permissions to apply.

**Format**: Sections represent profiles; entries are `<mode> <path-relative-to-home>`.

**Example**:
```ini
[base]
600 ssh/config
755 config/zsh

[desktop]
755 config/volume/init-volume.sh
```

---

### `fonts.ini`
**Purpose**: Lists font families to check for presence and install if missing.

**Format**: Single `[fonts]` section with font family names.

**Example**:
```ini
[fonts]
Noto Color Emoji
Source Code Pro
```

---

### `vscode-extensions.ini`
**Purpose**: Lists VS Code extensions to install.

**Format**: Single `[extensions]` section with extension IDs in `publisher.name` format.

**Example**:
```ini
[extensions]
github.copilot
ms-python.python
rust-lang.rust-analyzer
```

---

### `registry.ini`
**Purpose**: Configures Windows registry settings.

**Format**: Sections are registry paths; entries are `value-name = value` pairs.

**Example**:
```ini
[HKCU:\Console]
WindowSize = 0x00200078
FaceName = DejaVu Sans Mono for Powerline
QuickEdit = 1

[HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced]
Hidden = 1
HideFileExt = 0
```

## Adding New Configuration

### Adding a Package
Add to appropriate section in `packages.ini`:
```ini
[arch]
my-new-package
```

### Adding a Symlink
1. Place file in `symlinks/` directory
2. Add entry to appropriate section in `symlinks.ini`
3. Update `manifest.ini` if it belongs to a specific category

### Adding a New Profile
1. Define in `profiles.ini`:
   ```ini
   [my-profile]
   include=arch
   exclude=windows,desktop
   ```
2. Add sections to other config files as needed
3. Use with `--profile my-profile`

## Usage

Configuration files are automatically processed based on the selected profile:

**Linux**:
```bash
./dotfiles.sh -I --profile arch-desktop
```

**Windows**:
```powershell
.\dotfiles.ps1
```

All items defined in matching profile sections are automatically installed.

## See Also

- Main project README for installation instructions
- `src/linux/utils.sh` - INI parsing functions (`read_ini_section`, `should_include_profile_tag`)
- `src/linux/tasks.sh` - Task functions that process configuration files
