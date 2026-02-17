---
name: sparse-checkout-patterns
description: >
  Patterns for the sparse checkout and manifest system, which is central to the profile-based file filtering.
  Use when working with file exclusion categories, sparse checkout configuration, or manifest management.
metadata:
  author: sneivandt
  version: "1.0"
---

# Sparse Checkout Patterns

This skill documents the sparse checkout and manifest system that enables profile-based file filtering in the dotfiles repository.

## Overview

The sparse checkout system allows a single repository to support multiple environments (Linux headless, Linux desktop, Windows) by controlling which files are checked out based on the selected profile. This reduces disk usage and clutter by only checking out files relevant to the selected profile.

**Key Components:**
- **`conf/manifest.ini`**: Maps files to exclusion categories (windows, arch, desktop)
- **`conf/profiles.ini`**: Defines which categories each profile excludes
- **Sparse Checkout**: Git feature that controls which files appear in the working directory
- **Profile System**: Automatic configuration based on environment needs

## How It Works

1. **Profile Selection**: User selects a profile (e.g., `arch-desktop`, `base`, `windows`)
2. **Category Mapping**: Profile maps to exclude categories via `conf/profiles.ini`
3. **File Exclusion**: `conf/manifest.ini` lists which files belong to each exclude category
4. **Git Configuration**: Sparse checkout patterns are generated and applied via Git
5. **Working Directory Update**: Git removes excluded files from workspace (they remain in the repository)

## Manifest File Format

### Location and Purpose

**File**: `conf/manifest.ini`

Maps files in `symlinks/` directory to exclusion categories. Files listed here will be excluded when their category is excluded by the active profile.

### Section Headers: OR Logic

**IMPORTANT**: Unlike other configuration files, `manifest.ini` uses **OR logic** for comma-separated sections.

```ini
[windows]
AppData/

[arch]
config/pacman.conf

[arch,desktop]
config/xmonad/
```

**Logic Explanation:**
- `[windows]` - Excluded if `windows` category is excluded
- `[arch]` - Excluded if `arch` category is excluded
- `[arch,desktop]` - Excluded if **EITHER** `arch` **OR** `desktop` is excluded (not both required)

This ensures files shared by multiple categories are excluded when ANY category is excluded, preventing partial checkouts of related files.

**Contrast with Other Config Files:**
Most other configuration files (e.g., `packages.ini`, `symlinks.ini`) use **AND logic**:
- `[arch,desktop]` - Section processed only if **BOTH** `arch` **AND** `desktop` are active

### Path Format

Paths in `manifest.ini` are relative to the `symlinks/` directory:

```ini
# Directories must end with /
[desktop]
config/Code/
vscode-remote/

# Individual files listed explicitly
config/git/windows
```

When generating sparse checkout patterns, these paths are automatically prefixed with `symlinks/`.

### Base Profile Files

Files in the `[base]` profile category **do not** need to be listed in `manifest.ini` because base files are **never excluded** - they're included in all profiles.

**Only list files that should be excluded in certain profiles.**

## Sparse Checkout Configuration

### Implementation Location

**Implementation**: `cli/src/tasks/sparse_checkout.rs`

This function:
1. Resolves the profile to exclude categories
2. Applies OS auto-detection overrides (e.g., always exclude `windows` on Linux)
3. Reads excluded files from `manifest.ini` using `get_excluded_files()`
4. Generates sparse checkout patterns
5. Applies patterns via `git sparse-checkout` commands

### Pattern Generation

Sparse checkout patterns are generated in this format:

```bash
/*                           # Include all top-level files/directories
!/symlinks/AppData/          # Exclude Windows-specific directory
!/symlinks/config/git/windows # Exclude Windows git config
```

**Pattern Logic:**
1. Start with `/*` to include everything by default
2. Add `!/path` entries for each excluded file/directory from manifest
3. Git applies patterns to determine working directory contents

### Git Commands

```bash
# Initialize sparse checkout in no-cone mode (pattern-based, not directory-based)
git sparse-checkout init --no-cone

# Apply patterns from stdin
git sparse-checkout set --no-cone --stdin < patterns.txt

# View current patterns
git sparse-checkout list

# Force working directory to match sparse checkout rules
rm -rf symlinks/
git reset --hard HEAD
```

**Why `reset --hard`?**
Unlike `git checkout`, `git reset --hard` respects sparse checkout configuration and properly removes excluded files.

## Profile Integration

### Profile-to-Category Mapping

**File**: `conf/profiles.ini`

Defines which categories each profile excludes:

```ini
[base]
exclude = windows,arch,desktop

[arch]
exclude = windows,desktop

[arch-desktop]
exclude = windows

[windows]
exclude = arch,desktop
```

### Auto-Detection Overrides

The system automatically enforces compatibility overrides:

**Linux Systems:**
- Always exclude `windows` category (regardless of profile)
- Non-Arch Linux: Always exclude `arch` category

**Implementation:**
```bash
# In configure_sparse_checkout()
if [ "$IS_ARCH" -eq 0 ]; then
  # Not on Arch - always exclude arch
  exclude_categories="$exclude_categories,arch"
fi

# Windows always excluded on Linux
exclude_categories="$exclude_categories,windows"
```

This prevents incompatible operations even if someone manually selects an incompatible profile.

## Category Naming Conventions

### Existing Categories

- **`windows`**: Windows-specific files (PowerShell, AppData, registry configs)
- **`arch`**: Arch Linux-specific files (pacman, AUR, Arch-specific tools)
- **`desktop`**: Desktop environment files (GUI tools, window managers, fonts)
- **`base`**: Core shell configuration (never excluded, doesn't appear in manifest)

### Multi-Category Sections

Use comma-separated categories for files that belong to multiple contexts:

```ini
[arch,desktop]
config/xmonad/           # Arch desktop window manager
config/dunst/            # Arch desktop notifications
```

**Rule**: List a file under multiple categories when it should be excluded if **ANY** of those categories is excluded.

## Adding New Files to Manifest

### Step 1: Determine Exclusion Categories

Ask: "Which profiles should NOT have this file?"

**Examples:**
- PowerShell script → `[windows]` (exclude on Linux)
- Arch package config → `[arch]` (exclude on non-Arch systems)
- GUI tool config → `[desktop]` (exclude on headless systems)
- Window manager config → `[arch,desktop]` (exclude unless both arch AND desktop)

### Step 2: Add to Manifest

Add the file path under the appropriate section in `conf/manifest.ini`:

```ini
[windows]
# Add Windows-specific file
config/powershell/profile.ps1

[desktop]
# Add desktop-specific directory (must end with /)
config/alacritty/
```

### Step 3: Test Sparse Checkout

```bash
# Apply changes
./dotfiles.sh -I --profile base

# Verify file is excluded
ls symlinks/config/alacritty/  # Should not exist with base profile

# Test with appropriate profile
./dotfiles.sh -I --profile desktop
ls symlinks/config/alacritty/  # Should exist with desktop profile
```

### Step 4: Verify Profile Behavior

Test that the file behaves correctly across different profiles:

```bash
# Test exclusion
./dotfiles.sh -I --profile base --dry-run
git sparse-checkout list | grep alacritty  # Should show exclusion

# Test inclusion
./dotfiles.sh -I --profile desktop --dry-run
git sparse-checkout list | grep alacritty  # Should not show exclusion
```

## Common Patterns

### Pattern: Exclude Platform-Specific Directory

```ini
[windows]
AppData/                # Windows-specific application data
config/powershell/      # Windows PowerShell configuration
```

### Pattern: Exclude OS-Specific Tool Config

```ini
[arch]
config/pacman.conf      # Arch package manager
config/paru/            # Arch AUR helper
```

### Pattern: Exclude Desktop Environment Files

```ini
[desktop]
config/Code/            # VS Code settings
config/rofi/            # Application launcher
```

### Pattern: Exclude Arch Desktop-Specific

```ini
[arch,desktop]
config/xmonad/          # Window manager (needs both Arch and desktop)
config/dunst/           # Notification daemon
xinitrc                 # X11 initialization
```

## Relationship with Configuration Processing

**Important Distinction:**

1. **Sparse Checkout** (manifest.ini with OR logic)
   - Controls which files exist in working directory
   - Uses OR logic: `[arch,desktop]` = exclude if arch OR desktop excluded

2. **Configuration Processing** (packages.ini, symlinks.ini with AND logic)
   - Controls which items are installed/processed
   - Uses AND logic: `[arch,desktop]` = process only if arch AND desktop active

**Example:**
```ini
# manifest.ini (OR logic)
[arch,desktop]
config/xmonad/          # Exclude if EITHER arch OR desktop excluded

# packages.ini (AND logic)
[arch,desktop]
xmonad                  # Install only if BOTH arch AND desktop active
```

This ensures files are available when their corresponding packages are installed.

## Troubleshooting

### Files Not Being Excluded

1. **Check manifest syntax**: Ensure paths are relative to `symlinks/`
2. **Check directory trailing slash**: Directories must end with `/`
3. **Verify profile mapping**: Check `conf/profiles.ini` for correct excludes
4. **Review auto-detection**: System may override profile (e.g., Linux always excludes windows)

### Files Disappearing Unexpectedly

1. **Check manifest sections**: File may be listed under wrong category
2. **Review OR logic**: `[arch,desktop]` excludes if EITHER category excluded
3. **Verify sparse checkout state**: `git sparse-checkout list`
4. **Check for uncommitted changes**: Sparse checkout requires clean working directory

### Sparse Checkout Not Applying

1. **Git version**: Requires Git 2.25+ for sparse checkout support
2. **Not a Git repo**: Sparse checkout only works in Git repositories
3. **Dry-run mode**: Configuration is created but not applied in dry-run
4. **Check logs**: Review `~/.cache/dotfiles/install.log` for errors

## Rules for Agents

When working with sparse checkout and manifest:

1. **Always use OR logic** for manifest.ini sections (unlike other config files)
2. **Include trailing slash** for directories in manifest.ini
3. **Paths are relative** to `symlinks/` directory (prefix not included in manifest)
4. **Base files don't need listing** - only list files that should be excluded
5. **Test across profiles** after adding new manifest entries
6. **Use multi-category sections** `[arch,desktop]` when file should be excluded if ANY category is excluded
7. **Verify auto-detection** overrides - system enforces OS compatibility automatically
8. **Clean working directory** required before applying sparse checkout (uncommitted changes cause errors)
9. **Document category choices** when adding new files to manifest

## Related Skills and Documentation

- **`profile-system`** skill - Understanding profiles and selection
- **`ini-configuration`** skill - INI file format and parsing
- **`customization-guide`** skill - Adding new configuration items
- **`docs/PROFILES.md`** - User-facing profile documentation
- **`docs/ARCHITECTURE.md`** - System architecture overview

## Key Files

- **`conf/manifest.ini`** - File-to-category mappings
- **`conf/profiles.ini`** - Profile-to-exclude-category mappings
- **`cli/src/tasks/sparse_checkout.rs`** - Sparse checkout task implementation
- **`.git/info/sparse-checkout`** - Git's sparse checkout configuration (generated)
