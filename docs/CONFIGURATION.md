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
description = "Full desktop/workstation setup with GUI tools"
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
  "apm/config/base.yml",
  "apm/plugins/*",
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
- Example: `apm/plugins/*` links each direct child of `<repo>/symlinks/apm/plugins/` into `~/.apm/plugins/<child>`; APM uses those linked sources when deploying plugin primitives into Copilot, Codex, VS Code, and Copilot App targets
- On `uninstall`, managed symlinks are materialized: the source file or directory is copied into the target path so the link becomes a real file/directory. Real non-symlink targets are not overwritten.

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

**Note**: User unit files should exist in `symlinks/config/systemd/user/`; the task dependency graph ensures symlinks are created before units are enabled. System-scope units are enabled with `sudo systemctl` and must already be available to systemd.

---

### `pam.toml`
**Purpose**: Manages PAM service files under `/etc/pam.d`.

**Format**: Sections represent categories; entries are inline tables with `name` and exact `content`.

**Example**:
```toml
[arch-desktop]
services = [
  { name = "hyprlock", content = "# PAM configuration file for hyprlock\n# the 'login' configuration file (see /etc/pam.d/login)\n\nauth        include     login\n" },
]
```

**Note**: PAM files are authentication-critical system configuration and are written with `sudo` when needed. Service names must be plain file names, not paths.

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
  { key = "model",             value = "gpt-5.5"         },
  { key = "effortLevel",       value = "high"            },
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

**Purpose**: Defines custom script tasks that are scheduled during the Provision
phase. They run alongside other Provision tasks when dependencies allow; do not
rely on their position relative to built-in tasks unless the task model declares
an explicit dependency.

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
- `--check` and `--dryrun` are cooperative safety contracts: these modes execute
  the external script, so the script itself must not mutate state
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

## Configuration Cookbook

Use these small examples when making common changes. Section names are category
filters, so `[arch-desktop]` applies only when both `arch` and `desktop` are
active.

### Add a Package

Add the package to the narrowest appropriate section in `conf/packages.toml`:

```toml
[arch]
packages = ["my-new-package"]

[arch-desktop]
packages = [
  { name = "some-aur-package", aur = true },
]
```

Run `./dotfiles.sh install -d` to preview the package task before applying it.

### Add a Symlink

1. Place the source file or directory under `symlinks/`.
2. Add the source path to `conf/symlinks.toml`.
3. Add the path to `conf/manifest.toml` if it should be excluded from some
   sparse-checkout profiles.

```toml
# conf/symlinks.toml
[base]
symlinks = [
  "config/example/tool.toml",
]

# conf/manifest.toml
[desktop]
paths = [
  "config/example/",
]
```

The default target is `~/.<source>`, so `config/example/tool.toml` links to
`~/.config/example/tool.toml`. Use an explicit `{ source, target }` table for
paths that should not get a leading dot.

### Add a Windows Registry Key

Add a logical section to `conf/registry.toml` with a PowerShell registry path
and a `[section.values]` subtable:

```toml
[terminal]
path = 'HKCU:\Console'

[terminal.values]
FaceName = "Cascadia Mono"
QuickEdit = 1
```

Only `HKCU:\` paths are accepted. System-wide hives such as `HKLM:\` and
`HKCR:\` are outside the managed scope and are rejected.

Keep registry settings in Windows-only sections when they are tied to a
profile/category split.

### Add an Overlay Script

Overlay scripts live outside this repository and are useful for private or
machine-specific setup. In the overlay repository:

```toml
# <overlay>/conf/scripts.toml
[linux]
scripts = [
  { name = "Install private files", path = "scripts/private-files.sh" },
]
```

The script path is relative to the overlay root. Scripts should support
`--check`, `--dryrun`, `--remove`, and no-argument apply mode. Run once with
`--overlay /path/to/overlay`; the path is saved for future runs.

### Add a New Profile

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
description = "Full desktop/workstation setup with GUI tools"
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

## Implementation references

- `cli/src/runtime/config_support/toml_loader.rs` - generic TOML loader
- `cli/src/app/config/` - aggregate configuration and profiles
- `cli/src/domains/*/config/` - domain-owned models and validators

## Next read

- [Usage Guide](USAGE.md) - Running installs, dry-runs, overlays, and logs
- [Profile System](PROFILES.md) - Category matching and sparse checkout behavior
- [Architecture](ARCHITECTURE.md) - How configuration is loaded and processed
- [Testing](TESTING.md) - Validation commands for config changes
