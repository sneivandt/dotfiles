# Profile System

The profile system allows a single dotfiles repository to serve multiple environments without checking out unnecessary files. Profiles control which files are included via git sparse checkout and which configuration sections are processed.

## Available Profiles

| Profile | Description | Use Case |
|---------|-------------|----------|
| `base` | Minimal core shell configuration | Shared shell configs without OS-specific or desktop files |
| `arch` | Arch Linux headless | Server or headless Arch Linux system |
| `arch-desktop` | Arch Linux with desktop environment | Full Arch Linux workstation with GUI |
| `desktop` | Generic Linux desktop | Desktop tools on non-Arch Linux distributions |
| `windows` | Windows environment | Windows system (profile selection not available on Windows) |

## How Profiles Work

The profile system operates through several coordinated mechanisms:

### 1. Profile Selection

Profiles can be selected in three ways, in order of priority:

1. **Explicit CLI argument**: `--profile arch-desktop` (highest priority)
2. **Persisted profile**: Automatically read from `.git/config`
3. **Interactive prompt**: If no profile is set, you'll be prompted to select one

**Example - First time setup:**
```bash
./dotfiles.sh -I
# Prompts: "Select profile (1-5): "
# Selection is saved to .git/config for future runs
```

**Example - Subsequent runs:**
```bash
./dotfiles.sh -I
# Uses saved profile, no prompt
```

**Example - Override saved profile:**
```bash
./dotfiles.sh -I --profile base
# Uses 'base' and updates saved profile
```

### 2. Sparse Checkout

Git's sparse checkout feature controls which files are checked out to your working directory based on your profile. Files excluded by your profile are automatically removed from the workspace but remain in the repository.

**How it works:**
- Profile definitions in `conf/profiles.ini` specify which categories to exclude
- File categories are mapped in `conf/manifest.ini`
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
```ini
[arch]
base-devel
git
```
These items are processed when the `arch` category is NOT excluded.

**Multi-category sections:**
```ini
[arch,desktop]
xmonad
alacritty
```
These items require ALL listed categories to be active (AND logic).

### 4. OS Detection Overrides

The system applies automatic overrides to ensure compatibility:

- **Non-Arch Linux systems**: Always exclude `arch` category (even if profile includes it)
- **All Linux systems**: Always exclude `windows` category (even if profile includes it)

This prevents incompatible operations regardless of profile selection.

**Bypass for testing:**
```bash
./dotfiles.sh -I --profile arch --skip-os-detection
# Allows testing arch profile on non-Arch systems
```

### 5. Profile Persistence

Selected profiles are saved to `.git/config` for seamless reuse:

```bash
# Save profile
git config --local dotfiles.profile arch-desktop

# Read saved profile
git config --local --get dotfiles.profile
```

The installation script handles this automatically.

## Profile Definitions

Profiles are defined in `conf/profiles.ini`:

```ini
[base]
include=
exclude=windows,desktop,arch

[arch]
include=arch
exclude=windows,desktop

[arch-desktop]
include=arch,desktop
exclude=windows

[desktop]
include=desktop
exclude=windows,arch

[windows]
include=windows
exclude=arch,desktop
```

### Profile Syntax

- **Profile names**: Use hyphens (e.g., `[arch-desktop]`)
- **include**: Comma-separated list of categories to include
- **exclude**: Comma-separated list of categories to exclude

## Switching Profiles

When you switch profiles, the sparse checkout automatically adjusts:

```bash
# Switch from arch-desktop to base
./dotfiles.sh -I --profile base
# Desktop-specific files are automatically removed from workspace
# Symlinks to desktop files are removed
# Your selection is saved for future runs

# Switch back to arch-desktop
./dotfiles.sh -I --profile arch-desktop
# Desktop files are checked out again
# Desktop symlinks are created
```

## Creating Custom Profiles

You can create custom profiles for specific needs:

1. **Edit `conf/profiles.ini`:**
   ```ini
   [my-custom]
   include=arch
   exclude=windows,desktop
   ```

2. **Use your profile:**
   ```bash
   ./dotfiles.sh -I --profile my-custom
   ```

3. **Add profile-specific configuration:**
   - Add sections to `conf/packages.ini`, `conf/symlinks.ini`, etc.
   - Use section name `[my-custom]` or category combinations like `[arch,my-custom]`

## Profile Categories

Categories are logical groups used throughout the configuration system:

| Category | Purpose | Used In |
|----------|---------|---------|
| `windows` | Windows-specific configuration | Windows systems only |
| `arch` | Arch Linux-specific configuration | Arch Linux systems |
| `desktop` | Desktop/GUI configuration | Systems with GUI |

Custom categories can be created by:
1. Adding them to profile definitions in `conf/profiles.ini`
2. Creating corresponding sections in other `.ini` files
3. Mapping files to categories in `conf/manifest.ini`

## Advanced: Profile Dependencies

Profiles can have implicit dependencies through category combinations:

```ini
# This section requires BOTH arch AND desktop to be active
[arch,desktop]
alacritty
dunst
```

This allows fine-grained control over which items are installed in different environments.

## Examples

### Minimal Server Setup
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile base
# Only core shell configs, no desktop or OS-specific files
```

### Arch Linux Workstation
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile arch-desktop
# Full desktop environment with Arch packages
```

### Generic Linux Desktop (Ubuntu, Fedora, etc.)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile desktop
# Desktop tools without Arch-specific packages
```

### Windows System
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1
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
./dotfiles.sh -I --profile <your-profile>
```

### Desktop files missing on Arch
Use the `arch-desktop` profile, not `arch`:
```bash
./dotfiles.sh -I --profile arch-desktop
```

### Package installation failing
Check that the package is defined in a section matching your active profile:
```bash
# Enable verbose mode to see what sections are being processed
./dotfiles.sh -I -v
```

## See Also

- [Configuration Reference](CONFIGURATION.md) - Details on configuration file formats
- [Customization Guide](CUSTOMIZATION.md) - Adding files, packages, and profiles
- [Usage Guide](USAGE.md) - Detailed usage examples
