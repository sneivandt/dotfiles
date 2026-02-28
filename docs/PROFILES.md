# Profile System

The profile system allows a single dotfiles repository to serve multiple environments without checking out unnecessary files. Profiles control which files are included via git sparse checkout and which configuration sections are processed.

## Available Profiles

Profiles are defined in `conf/profiles.toml` and control the `desktop` role category. Platform categories (`linux`, `windows`, `arch`) are auto-detected and cannot be selected manually.

| Profile | Description | Use Case |
|---------|-------------|----------|
| `base` | Minimal core shell configuration (excludes `desktop`) | Shared shell configs without desktop-specific files |
| `desktop` | Full configuration including desktop tools (includes `desktop`) | Workstation with GUI, VS Code, fonts, etc. |

### Auto-Detected Platform Categories

These categories are determined automatically based on the running OS and are always applied — they are not profiles you select:

| Category | When Active | Effect |
|----------|-------------|--------|
| `linux` | Running on Linux | Includes Linux-specific packages, shell config, systemd units |
| `windows` | Running on Windows | Includes Windows packages, registry settings, git config |
| `arch` | Running on Arch Linux | Includes pacman/AUR packages, Arch-specific config |

## How Profiles Work

The profile system operates through several coordinated mechanisms:

### 1. Profile Selection

Profiles can be selected in three ways, in order of priority:

1. **Explicit CLI argument**: `-p, --profile desktop` (highest priority)
2. **Persisted profile**: Automatically read from `.git/config`
3. **Interactive prompt**: If no profile is set, you'll be prompted to select one

**Example - First time setup:**
```bash
./dotfiles.sh install
# Prompts: "Select profile (1-2): "
# Selection is saved to .git/config for future runs
```

**Example - Subsequent runs:**
```bash
./dotfiles.sh install
# Uses saved profile, no prompt
```

**Example - Override saved profile:**
```bash
./dotfiles.sh install -p base
# Uses 'base' and updates saved profile
```

### 2. Sparse Checkout

Git's sparse checkout feature controls which files are checked out to your working directory based on your profile. Files excluded by your profile are automatically removed from the workspace but remain in the repository.

**How it works:**
- Profile definitions in `conf/profiles.toml` specify which categories to exclude
- File categories are mapped in `conf/manifest.toml`
- Git sparse checkout rules are automatically configured
- Excluded files don't clutter your workspace

**Example:**
```bash
# Check what files are included in sparse checkout
git sparse-checkout list
```

### 3. Configuration Processing

Configuration files (packages, symlinks, units, etc.) use section headers to determine which items are processed:

**Single category sections:**
```toml
[arch]
packages = [
  "base-devel",
  "git",
]
```
These items are processed when the `arch` category is NOT excluded.

**Multi-category sections:**
```toml
[arch-desktop]
packages = [
  "xmonad",
  "alacritty",
]
```
These items require ALL listed categories to be active (AND logic).

### 4. OS Detection Overrides

The system applies automatic overrides to ensure compatibility:

- **Non-Arch Linux systems**: Always exclude `arch` category (even if profile includes it)
- **All Linux systems**: Always exclude `windows` category (even if profile includes it)

This prevents incompatible operations regardless of profile selection.

### 5. Profile Persistence

Selected profiles are saved to `.git/config` for seamless reuse:

```bash
# Save profile
git config --local dotfiles.profile desktop

# Read saved profile
git config --local --get dotfiles.profile
```

The installation script handles this automatically.

## Profile Definitions

Profiles are defined in `conf/profiles.toml`:

```toml
[base]
include = []
exclude = ["desktop"]

[desktop]
include = ["desktop"]
exclude = []
```

Platform categories (`linux`, `windows`, `arch`) are not defined in profiles.toml — they are auto-detected at runtime based on the operating system.

### Profile Syntax

- **Profile names**: Section headers in `profiles.toml` (e.g., `[base]`, `[desktop]`)
- **include**: Array of categories to include
- **exclude**: Array of categories to exclude

## Switching Profiles

When you switch profiles, the sparse checkout automatically adjusts:

```bash
# Switch from desktop to base
./dotfiles.sh install -p base
# Desktop-specific files are automatically removed from workspace
# Symlinks to desktop files are removed
# Your selection is saved for future runs

# Switch back to desktop
./dotfiles.sh install -p desktop
# Desktop files are checked out again
# Desktop symlinks are created
```

## Creating Custom Profiles

You can create custom profiles for specific needs:

1. **Edit `conf/profiles.toml`:**
   ```toml
   [my-custom]
   include = ["mycategory"]
   exclude = []
   ```

2. **Use your profile:**
   ```bash
   ./dotfiles.sh install -p my-custom
   ```

3. **Add profile-specific configuration:**
   - Add sections to `conf/packages.toml`, `conf/symlinks.toml`, etc.
   - Use section name `[my-custom]` or multi-category like `[arch-my-custom]`

## Profile Categories

Categories are logical groups used throughout the configuration system:

| Category | Purpose | Used In |
|----------|---------|---------|
| `linux` | Linux-specific configuration | Linux systems (auto-detected) |
| `windows` | Windows-specific configuration | Windows systems (auto-detected) |
| `arch` | Arch Linux-specific configuration | Arch Linux systems (auto-detected) |
| `desktop` | Desktop/GUI configuration | Systems using `desktop` profile |

Custom categories can be created by:
1. Adding them to profile definitions in `conf/profiles.toml`
2. Creating corresponding sections in other `.toml` files
3. Mapping files to categories in `conf/manifest.toml`

## Advanced: Profile Dependencies

Profiles can have implicit dependencies through category combinations:

```toml
# This section requires BOTH arch AND desktop to be active
[arch-desktop]
packages = [
  "alacritty",
  "dunst",
]
```

This allows fine-grained control over which items are installed in different environments.

## Examples

### Minimal Server Setup
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p base
# Only core shell configs, no desktop or OS-specific files
```

### Arch Linux Workstation
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Full desktop environment with Arch packages
```

### Generic Linux Desktop (Ubuntu, Fedora, etc.)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Desktop tools without Arch-specific packages
```

### Windows System
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
# Windows profile is automatically used
```

## Troubleshooting

### Wrong files checked out
```bash
# Check current profile
git config --local --get dotfiles.profile

# Check sparse checkout rules
git sparse-checkout list

# Reapply profile
./dotfiles.sh install -p <your-profile>
```

### Desktop files missing on Arch
Use the `desktop` profile (the `arch` category is auto-detected):
```bash
./dotfiles.sh install -p desktop
```

### Package installation failing
Check that the package is defined in a section matching your active profile:
```bash
# Enable verbose mode to see what sections are being processed
./dotfiles.sh install -v
```

## See Also

- [Configuration Reference](CONFIGURATION.md) - Details on configuration file formats
- [Usage Guide](USAGE.md) - Detailed usage examples
