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

**Format**: Sections represent categories; entries are source paths relative to `symlinks/` (without leading dot).

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
  { source = "apm/plugins/*", target = ".copilot/skills/*" },
  "config/git/windows",
]
```

**How it works**:
- Source files are located in `symlinks/<path>` at repository root
- Source and explicit target paths must be relative and must not contain `..` components
- By default, targets are created at `~/.<path>` (a dot is prepended)
- For paths that must not receive a dot prefix (e.g. Windows `AppData\`, `Documents\`), use `{ source = "...", target = "..." }` to specify the target explicitly
- Directory globs are supported when `*` is a complete path segment. Each matched source segment is substituted into the corresponding `*` in `target`.
- Globs are expanded during config load and preserve overlay ownership, so overlay entries link back to `<overlay>/symlinks/...` rather than the main repo.
- Example: `config/nvim` → `~/.config/nvim` symlinked to `<repo>/symlinks/config/nvim`
- Example: `{ source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" }` → `~/AppData/Roaming/Code/User/settings.json`
- Example: `{ source = "apm/plugins/*", target = ".copilot/skills/*" }` links each direct child of `<repo>/symlinks/apm/plugins/` into `~/.copilot/skills/<child>`

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
**Purpose**: Lists systemd user or system units to enable and start.

**Format**: Sections represent categories; entries are unit filenames or inline tables with `name` and `scope`. Plain strings default to `scope = "user"`; explicit scope must be `"user"` or `"system"`.

**Example**:
```toml
[linux]
units = ["clean-home-tmp.timer"]

[arch-desktop]
units = [
  "dunst.service",
  "volume.service",
  { name = "sshd.service", scope = "system" },
]
```

**Note**: User unit files should exist in `symlinks/config/systemd/user/` and be symlinked before enabling. System-scope units are enabled with `sudo systemctl` and must already be available to systemd.

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

### `copilot.toml`
**Purpose**: Converges selected keys inside the GitHub Copilot CLI settings file
(`~/.copilot/settings.json`).

**Format**: Sections represent categories; entries are inline tables with `key` and
`value` fields. The `key` is a dot-separated path into the JSON document, and
`value` is any TOML scalar, array, or inline table (converted to JSON).

**Example**:
```toml
[base]
settings = [
  { key = "model",             value = "claude-opus-4.8" },
  { key = "beep",              value = false             },
  { key = "footer.showBranch", value = true              },
]
```

**How it works**:
- Only the listed keys are managed; every other key in `settings.json` is
  preserved on write. This makes it safe to apply against a *volatile* file the
  Copilot CLI also rewrites at runtime (e.g. after `/model` or `/theme`).
- Dotted keys (such as `footer.showBranch`) target nested values individually
  without clobbering sibling keys.
- A key is only rewritten when its current value drifts from the declared one,
  so up-to-date runs make no changes.
- Volatile, CLI-managed state (`enabledPlugins`, `extraKnownMarketplaces`,
  `sessionSync`, login data) is intentionally left out. Plugins are managed
  declaratively via APM instead.

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
  - **`--check`**: Verify state (exit 0 = correct, exit 1 = needs apply, any other non-zero exit = check failure)
  - **`--dryrun`**: Preview what apply would do without mutating state
  - **`--remove`**: Undo the applied state
- `.ps1` scripts are invoked via `pwsh` when available; Windows falls back to `powershell`, while non-Windows platforms require `pwsh`
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
