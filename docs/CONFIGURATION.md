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
description = "Full graphical desktop (Arch: Hyprland/Wayland)"
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

**How exclusions work**:
- A section is excluded only if ALL of its category tags match the excluded set (AND logic)
- `[arch-desktop]` means "exclude only if both arch AND desktop are excluded"
- This is the same AND logic used by all other configuration files

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
  "config/hypr/hyprland.conf",
  "config/dunst/dunstrc",
]

[windows]
symlinks = [
  { source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" },
  "config/git/windows",
]
```

**How it works**:
- Source files are located in `symlinks/<path>` at repository root
- By default, targets are created at `~/.<path>` (a dot is prepended)
- For paths that must not receive a dot prefix (e.g. Windows `AppData\`, `Documents\`), use `{ source = "...", target = "..." }` to specify the target explicitly
- Example: `config/nvim` → `~/.config/nvim` symlinked to `<repo>/symlinks/config/nvim`
- Example: `{ source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" }` → `~/AppData/Roaming/Code/User/settings.json`

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
  "volume.service",
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

**Directory behaviour**: When `path` points to a directory, the requested mode
is applied recursively with directory execute bits preserved for traversal, but
regular files in that tree have execute bits cleared unless they are configured
as standalone entries.

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

### `copilot-plugins.toml`
**Purpose**: Lists GitHub Copilot CLI plugins to install from marketplaces.

**Format**: Sections represent categories; entries are inline tables with marketplace metadata.

**Example**:
```toml
[base]
plugins = [
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-diag" },
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-msbuild" },
]
```

**How it works**:
- The task ensures the marketplace is registered with `gh copilot plugin marketplace add`
- Plugins are installed with `gh copilot plugin install <plugin>@<marketplace_name>`
- Installed plugins are detected via `gh copilot plugin list`
- Requires GitHub CLI with the Copilot extension (`gh copilot`) on `PATH`

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

---

### `git-config.toml`
**Purpose**: Applies global git configuration settings.

**Format**: Sections represent categories; entries are inline tables with `key` and
`value` fields.

**Example**:
```toml
[windows]
settings = [
  { key = "core.autocrlf", value = "false" },
  { key = "core.symlinks", value = "true" },
  { key = "credential.helper", value = "manager" },
]
```

**How it works**:
- The `key` should use standard git `section.name` syntax such as `core.autocrlf`
- The value is written with `git config --global`
- The current repository uses this for Windows-specific git defaults

---

## Overlay Configuration

An **overlay repository** provides private configuration extensions that are
merged with the main dotfiles config at runtime.  Any standard `conf/*.toml`
file placed in the overlay’s `conf/` directory is loaded and its entries are
appended to the main config lists.

The overlay path is resolved from (in priority order):
1. `--overlay` CLI flag
2. `DOTFILES_OVERLAY` environment variable
3. `dotfiles.overlay` in the repo’s local git config

### `scripts.toml` (Overlay)

**Purpose**: Defines custom script tasks that run during the Apply phase.

**Location**: `<overlay>/conf/scripts.toml`

**Format**: Sections represent categories; entries are inline tables with `name`, `path`, and optional `description`.

**Example**:
```toml
[linux]
scripts = [
  { name = "Setup work SSH", path = "scripts/ssh.sh" },
]
```

**How it works**:
- Each entry becomes a separate task that appears in the output like any built-in task
- The `path` is relative to the overlay repository root
- Scripts follow a convention-based interface:
  - **No arguments**: Apply the desired state
  - **`--check`**: Verify state (exit 0 = correct, non-zero = needs apply)
  - **`--remove`**: Undo the applied state
- `.ps1` scripts are invoked via `powershell` (Windows) or `pwsh` (other platforms)
- `.sh` scripts are invoked via `sh`
- Scripts run with `-NonInteractive` to prevent interactive prompts

### Overlay TOML File Merging

Any TOML file that exists in both the main `conf/` and the overlay `conf/`
directory is merged by **appending** the overlay entries to the main config.
This works for all standard config types:

```
<overlay>/conf/packages.toml      → appended to packages list
<overlay>/conf/symlinks.toml      → appended to symlinks list
<overlay>/conf/vscode-extensions.toml → appended to extensions list
... etc.
```

The same category filtering rules apply to overlay sections.

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
description = "Full graphical desktop (Arch: Hyprland/Wayland)"
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
  "hyprland",
  "waybar",
]

# symlinks.toml
[arch-desktop]
symlinks = [
  "config/hypr/hyprland.conf",
  "config/dunst/dunstrc",
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
