# Configuration Files

The `conf/` directory contains all configuration files for the dotfiles management system. All files use TOML format with section headers for organization.

## Overview

The configuration system is based on **profiles** that control:
- Which files are checked out via git sparse checkout
- Which packages, units, and other components are installed
- Which symlinks are created

Configuration files are processed automatically based on the active profile. Items defined in matching profile sections are automatically installed without requiring additional flags.

## Configuration File Format

All configuration files use TOML format:

```toml
# Comments start with #
[section-name]
items = [
  "entry-one",
  "entry-two",
]

[another-section]
items = ["more-entries"]
```

**Key rules:**
- **Profile names** (in profiles.toml only): `[base]`, `[desktop]`
- **Section names** (in all other .toml files) use hyphen-separated categories: `[arch-desktop]`
  - This indicates the section requires ALL listed categories to be active (AND logic)
  - Example: `[arch-desktop]` is only processed when both `arch` AND `desktop` are not excluded
  - **Exception**: `manifest.toml` uses OR logic—`[arch-desktop]` means exclude if arch OR desktop is excluded
- Empty lines and comments starting with `#` are ignored
- Only sections whose required categories match the active profile are processed
- `registry.toml` uses a nested structure: logical section names with a `path` key and `[section.values]` subtable

## Available Profiles

Profiles are defined in `profiles.toml` and control the `desktop` role category:

- **`base`**: Minimal core shell configuration (excludes `desktop`)
- **`desktop`**: Full configuration including desktop tools (includes `desktop`)

Platform categories (`linux`, `windows`, `arch`) are auto-detected based on the running OS and always applied automatically — they are not user-selectable profiles.

## Configuration Files

### `profiles.toml`
**Purpose**: Defines available profiles and their include/exclude categories.

**Format**: Each profile specifies categories to include or exclude.

**Example**:
```toml
[desktop]
description = "Full graphical desktop (Arch + X11)"
include = ["desktop"]
exclude = []
```

**Categories**:
- `linux`: Linux-specific configuration (auto-detected)
- `windows`: Windows-specific configuration (auto-detected)
- `arch`: Arch Linux-specific configuration (auto-detected)
- `desktop`: Desktop/GUI configuration (controlled by profile selection)

---

### `manifest.toml`
**Purpose**: Maps files and directories to categories for git sparse checkout exclusion.

**Format**: Sections represent categories; entries are paths relative to `symlinks/` directory root.

**Example**:
```toml
[desktop]
paths = [
  "config/Code/",
  "config/Code - Insiders/",
]

[arch-desktop]
paths = [
  "config/dunst/",
]
```

**Important: OR Logic for Exclusions**:
- Unlike other configuration files, manifest.toml uses **OR logic** for multi-category sections
- `[arch-desktop]` means "exclude if arch OR desktop is excluded" (not both required)
- This ensures files common to multiple categories are excluded if ANY of those categories is excluded
- Other config files use AND logic: `[arch-desktop]` means "include only if BOTH arch AND desktop are active"

---

### `symlinks.toml`
**Purpose**: Defines symlinks to create in `$HOME`.

**Format**: Sections represent categories; entries are paths relative to `$HOME` (without leading dot).

**Example**:
```toml
[base]
symlinks = [
  "bashrc",
  "config/git/config",
  "config/nvim",
]

[arch-desktop]
symlinks = [
  "xinitrc",
  "config/xmonad/xmonad.hs",
]
```

**How it works**:
- Source files are located in `symlinks/<path>` at repository root
- Targets are created at `~/.<path>`
- Example: `config/nvim` → `~/.config/nvim` symlinked to `<repo>/symlinks/config/nvim`

---

### `packages.toml`
**Purpose**: Lists system packages to install via package manager.
- **Linux**: Uses `pacman` (Arch Linux) and `paru` (AUR)
- **Windows**: Uses `winget`

**Format**: Sections represent categories; entries are package names as strings. AUR packages use an inline table with `aur = true`.

**Example**:
```toml
[arch]
packages = [
  "git",
  "base-devel",
  { name = "powershell-bin", aur = true },
]

[arch-desktop]
packages = [
  "alacritty",
  { name = "spotify", aur = true },
]
```

---

### `systemd-units.toml`
**Purpose**: Lists systemd user units to enable and start.

**Format**: Sections represent categories; entries are unit filenames.

**Example**:
```toml
[linux]
units = ["clean-home-tmp.timer"]

[arch-desktop]
units = [
  "dunst.service",
  "picom.service",
]
```

**Note**: Unit files should exist in `symlinks/config/systemd/user/` and be symlinked before enabling.

---

### `chmod.toml`
**Purpose**: Specifies file permissions to apply.

**Format**: Sections represent categories; entries are inline tables with `mode` and `path` keys.

**Example**:
```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/zsh" },
]

[arch-desktop]
permissions = [
  { mode = "755", path = "config/volume/init-volume.sh" },
]
```

---

### `vscode-extensions.toml`
**Purpose**: Lists VS Code extensions to install.

**Format**: Sections represent categories; entries are extension IDs in `publisher.name` format.

**Example**:
```toml
[desktop]
extensions = [
  "github.copilot-chat",
  "ms-python.python",
  "rust-lang.rust-analyzer",
]

[windows]
extensions = ["ms-vscode-remote.remote-wsl"]
```

---

### `copilot-skills.toml`
**Purpose**: Lists GitHub Copilot CLI skill folders to download and install.

**Format**: Sections represent categories; entries are GitHub folder URLs.

**Example**:
```toml
[base]
skills = [
  "https://github.com/github/awesome-copilot/blob/main/skills/azure-devops-cli",
  "https://github.com/microsoft/skills/blob/main/.github/skills/azure-identity-dotnet",
]
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

### `registry.toml`
**Purpose**: Configures Windows registry settings.

**Format**: Logical section names with a `path` key (the registry path) and a `[section.values]` subtable for key-value pairs.

**Example**:
```toml
[console]
path = 'HKCU:\Console'

[console.values]
WindowSize = 0x00200078
FaceName = "Cascadia Mono"
QuickEdit = 1

[psreadline]
path = 'HKCU:\Console\PSReadLine'

[psreadline.values]
NormalForeground = 0xF
```

## Adding New Configuration

### Adding a Package
Add to appropriate section in `packages.toml`:
```toml
[arch]
packages = ["my-new-package"]
```

### Adding a Symlink
1. Place file in `symlinks/` directory
2. Add entry to appropriate section in `symlinks.toml`
3. Update `manifest.toml` if it belongs to a specific category

### Adding a New Profile
1. Define in `profiles.toml`:
   ```toml
   [my-profile]
   include = ["mycategory"]
   exclude = ["desktop"]
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

```toml
# profiles.toml
[base]
description = "Core shell environment, no desktop GUI"
include = []
exclude = ["desktop"]

# symlinks.toml
[base]
symlinks = [
  "bashrc",
  "vimrc",
  "config/git/config",
]

# packages.toml
[arch]
packages = [
  "git",
  "vim",
]
```

### Example: Desktop Profile with Multiple Categories

Configuration requiring multiple categories (on an Arch Linux system with the `desktop` profile selected):

```toml
# profiles.toml
[desktop]
description = "Full graphical desktop (Arch + X11)"
include = ["desktop"]
exclude = []

# packages.toml — arch and desktop categories are both active
[arch]
packages = [
  "base-devel",
  "pacman-contrib",
]

[desktop]
packages = ["code"]

[arch-desktop]
packages = [
  "xorg-server",
  "xmonad",
]

# symlinks.toml
[arch-desktop]
symlinks = [
  "xinitrc",
  "config/xmonad/xmonad.hs",
]
```

### Example: Conditional Package Installation

Install packages only when specific categories are active:

```toml
# packages.toml
[arch]
# Always installed on Arch
packages = [
  "git",
  "base-devel",
  { name = "powershell-bin", aur = true },
]

[arch-desktop]
# Only on Arch with desktop
packages = [
  "alacritty",
  "dunst",
  { name = "chromium-widevine", aur = true },
  { name = "spotify", aur = true },
]
```

## See Also

- [Usage Guide](USAGE.md) - How to use configuration files
- [Profile System](PROFILES.md) - Understanding profile filtering
- [Architecture](ARCHITECTURE.md) - How configuration is processed
- `cli/src/config/toml_loader.rs` - TOML loader implementation
- `cli/src/config/` - Configuration loader modules
