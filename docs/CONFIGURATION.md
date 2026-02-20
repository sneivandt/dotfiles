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
- **Profile names** (in profiles.ini only): `[base]`, `[desktop]`
- **Section names** (in all other .ini files) use comma-separated categories: `[arch,desktop]`
  - This indicates the section requires ALL listed categories to be active (AND logic)
  - Example: `[arch,desktop]` is only processed when both `arch` AND `desktop` are not excluded
  - **Exception**: `manifest.ini` uses OR logic—`[arch,desktop]` means exclude if arch OR desktop is excluded
- Empty lines and comments starting with `#` are ignored
- Only sections whose required categories match the active profile are processed
- `registry.ini` uses `key = value` format with registry paths as sections (Windows-only)

## Available Profiles

Profiles are defined in `profiles.ini` and control the `desktop` role category:

- **`base`**: Minimal core shell configuration (excludes `desktop`)
- **`desktop`**: Full configuration including desktop tools (includes `desktop`)

Platform categories (`linux`, `windows`, `arch`) are auto-detected based on the running OS and always applied automatically — they are not user-selectable profiles.

## Configuration Files

### `profiles.ini`
**Purpose**: Defines available profiles and their include/exclude categories.

**Format**: Each profile specifies categories to include or exclude.

**Example**:
```ini
[desktop]
include=desktop
exclude=
```

**Categories**:
- `linux`: Linux-specific configuration (auto-detected)
- `windows`: Windows-specific configuration (auto-detected)
- `arch`: Arch Linux-specific configuration (auto-detected)
- `desktop`: Desktop/GUI configuration (controlled by profile selection)

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
- AUR packages are prefixed with `aur:` and handled by `paru`
- Arch Linux sections (e.g., `[arch]`, `[arch,desktop]`) are handled by the standard package manager (`pacman`)
- Windows sections (e.g., `[windows]`) are handled by `winget`

**Example**:
```ini
[arch]
git
base-devel
aur:powershell-bin

[arch,desktop]
alacritty
aur:spotify
```

---

### `systemd-units.ini`
**Purpose**: Lists systemd user units to enable and start.

**Format**: Sections represent categories; entries are unit filenames.

**Example**:
```ini
[linux]
clean-home-tmp.timer

[arch,desktop]
dunst.service
picom.service
```

**Note**: Unit files should exist in `symlinks/config/systemd/user/` and be symlinked before enabling.

---

### `chmod.ini`
**Purpose**: Specifies file permissions to apply.

**Format**: Sections represent categories; entries are `<mode> <path-relative-to-home>`.

**Example**:
```ini
[linux]
600 ssh/config
755 config/zsh

[arch,desktop]
755 config/volume/init-volume.sh
```

---

### `vscode-extensions.ini`
**Purpose**: Lists VS Code extensions to install.

**Format**: Sections represent categories; entries are extension IDs in `publisher.name` format.

**Example**:
```ini
[desktop]
github.copilot
ms-python.python
rust-lang.rust-analyzer

[windows]
ms-vscode-remote.remote-wsl
```

---

### `copilot-skills.ini`
**Purpose**: Lists GitHub Copilot CLI skill folders to download and install.

**Format**: Sections represent profiles; entries are GitHub folder URLs.

**Example**:
```ini
[base]
https://github.com/github/awesome-copilot/blob/main/skills/azure-devops-cli
https://github.com/microsoft/skills/blob/main/.github/skills/azure-identity-dotnet

[desktop]
https://github.com/example/skills/blob/main/skills/web-dev-helper
```

**How it works**:
- Skills are downloaded to `~/.copilot/skills/` directory
- Each URL points to a folder in a GitHub repository
- The entire folder (including subdirectories) is downloaded
- Folder name is extracted from the URL path
- Requires `gh` CLI for GitHub Copilot functionality

**URL format**: Both `/blob/` and `/tree/` URLs are supported:
- `https://github.com/owner/repo/blob/branch/path/to/folder`
- `https://github.com/owner/repo/tree/branch/path/to/folder`

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
   include=mycategory
   exclude=desktop
   ```
2. Add sections to other config files as needed
3. Use with `-p my-profile`

## Usage

Configuration files are automatically processed based on the selected profile:

**Linux**:
```bash
./dotfiles.sh install -p desktop
```

**Windows**:
```powershell
.\dotfiles.ps1 install -p desktop
```

All items defined in matching profile sections are automatically installed.

## Examples

### Example: Base Profile Configuration

A minimal setup with core shell configuration:

```ini
# profiles.ini
[base]
include=
exclude=desktop

# symlinks.ini
[base]
bashrc
vimrc
config/git/config

# packages.ini
[base]
git
vim
```

### Example: Desktop Profile with Multiple Categories

Configuration requiring multiple categories (on an Arch Linux system with the `desktop` profile selected):

```ini
# profiles.ini
[desktop]
include=desktop
exclude=

# packages.ini — arch and desktop categories are both active
[arch]
base-devel
pacman-contrib

[desktop]
code

[arch,desktop]
xorg-server
xmonad

# symlinks.ini
[arch,desktop]
xinitrc
config/xmonad/xmonad.hs
```

### Example: Conditional Package Installation

Install packages only when specific categories are active:

```ini
# packages.ini
[arch]
# Always installed on Arch
git
base-devel
aur:powershell-bin

[arch,desktop]
# Only on Arch with desktop
alacritty
dunst
aur:chromium-widevine
aur:spotify
```

## See Also

- [Usage Guide](USAGE.md) - How to use configuration files
- [Profile System](PROFILES.md) - Understanding profile filtering
- [Architecture](ARCHITECTURE.md) - How configuration is processed
- `cli/src/config/ini.rs` - INI parser implementation
- `cli/src/config/` - Configuration loader modules
