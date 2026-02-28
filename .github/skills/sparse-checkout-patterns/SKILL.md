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
- **`conf/manifest.toml`**: Maps files to exclusion categories (windows, arch, desktop)
- **`conf/profiles.toml`**: Defines which categories each profile excludes
- **Sparse Checkout**: Git feature that controls which files appear in the working directory
- **Profile System**: Automatic configuration based on environment needs

## How It Works

1. **Profile Selection**: User selects a profile (e.g., `desktop`, `base`)
2. **Category Mapping**: Profile maps to exclude categories via `conf/profiles.toml`
3. **File Exclusion**: `conf/manifest.toml` lists which files belong to each exclude category
4. **Git Configuration**: Sparse checkout patterns are generated and applied via Git
5. **Working Directory Update**: Git removes excluded files from workspace (they remain in the repository)

## Manifest File Format

### Location and Purpose

**File**: `conf/manifest.toml`

Maps files in `symlinks/` directory to exclusion categories. Files listed here will be excluded when their category is excluded by the active profile.

### Section Headers: OR Logic

**IMPORTANT**: Unlike other configuration files, `manifest.toml` uses **OR logic** for multi-category sections.

```toml
[windows]
paths = ["AppData/"]

[arch]
paths = ["config/pacman.conf"]

["arch.desktop"]
paths = ["config/xmonad/"]
```

**Logic Explanation:**
- `[windows]` - Excluded if `windows` category is excluded
- `[arch]` - Excluded if `arch` category is excluded
- `["arch.desktop"]` - Excluded if **EITHER** `arch` **OR** `desktop` is excluded (not both required)

This ensures files shared by multiple categories are excluded when ANY category is excluded, preventing partial checkouts of related files.

**Contrast with Other Config Files:**
Most other configuration files (e.g., `packages.toml`, `symlinks.toml`) use **AND logic**:
- `["arch.desktop"]` - Section processed only if **BOTH** `arch` **AND** `desktop` are active

### Path Format

Paths in `manifest.toml` are relative to the `symlinks/` directory:

```toml
# Directories must end with /
[desktop]
paths = [
  "config/Code/",
  "vscode-remote/",
]

# Individual files listed explicitly
[windows]
paths = ["config/git/windows"]
```

When generating sparse checkout patterns, these paths are automatically prefixed with `symlinks/`.

### Base Profile Files

Files in the `[base]` profile category **do not** need to be listed in `manifest.toml` because base files are **never excluded** - they're included in all profiles.

**Only list files that should be excluded in certain profiles.**

## Sparse Checkout Configuration

### Implementation Location

**Implementation**: `cli/src/tasks/sparse_checkout.rs`

This function:
1. Resolves the profile to exclude categories
2. Applies OS auto-detection overrides (e.g., always exclude `windows` on Linux)
3. Reads excluded files from `manifest.toml` using `get_excluded_files()`
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
# Initialize sparse checkout in cone mode first, then switch to no-cone
git sparse-checkout init --cone
git sparse-checkout init --no-cone

# View current patterns
git sparse-checkout list

# Apply patterns by writing to .git/info/sparse-checkout, then:
git read-tree -mu HEAD
```

**Why `read-tree -mu HEAD`?**
The implementation writes patterns directly to `.git/info/sparse-checkout` and then uses `git read-tree -mu HEAD` to update the working directory. Before `read-tree`, it runs `git checkout HEAD -- <excluded-files>` to reset any dirty excluded files so `read-tree` doesn't fail with "not uptodate" errors.

## Profile Integration

### Profile-to-Category Mapping

**File**: `conf/profiles.toml`

Defines which categories each profile excludes:

```toml
[base]
excludes = ["desktop"]

[desktop]
excludes = []
```

Platform categories (`linux`, `windows`, `arch`) are auto-detected, not defined in profiles.

### Auto-Detection Overrides

The system automatically enforces compatibility overrides:

**Linux Systems:**
- Always exclude `windows` category (regardless of profile)
- Non-Arch Linux: Always exclude `arch` category

**Implementation** (in `cli/src/tasks/sparse_checkout.rs`):

The Rust engine calls `Platform::excludes_category()` to determine which categories are incompatible with the current OS. These overrides are applied before generating sparse checkout patterns, preventing incompatible files from appearing even with a manually-selected profile.

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
- Window manager config → `["arch.desktop"]` (exclude unless both arch AND desktop)

### Step 2: Add to Manifest

Add the file path under the appropriate section in `conf/manifest.toml`:

```toml
[windows]
paths = [
  # Add Windows-specific file
  "config/powershell/profile.ps1",
]

[desktop]
paths = [
  # Add desktop-specific directory (must end with /)
  "config/alacritty/",
]
```

### Step 3: Test Sparse Checkout

```bash
# Apply changes
./dotfiles.sh install --profile base

# Verify file is excluded
ls symlinks/config/alacritty/  # Should not exist with base profile

# Test with appropriate profile
./dotfiles.sh install --profile desktop
ls symlinks/config/alacritty/  # Should exist with desktop profile
```

### Step 4: Verify Profile Behavior

Test that the file behaves correctly across different profiles:

```bash
# Test exclusion
./dotfiles.sh install --profile base --dry-run
git sparse-checkout list | grep alacritty  # Should show exclusion

# Test inclusion
./dotfiles.sh install --profile desktop --dry-run
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
### Pattern: Exclude Arch-Specific System Files

```toml
[arch]
paths = [
  "config/pacman.conf",      # Arch package manager
  "config/paru/",            # Arch AUR helper
]
```

### Pattern: Exclude Desktop Environment Files

```toml
[desktop]
paths = [
  "config/Code/",            # VS Code settings
  "config/rofi/",            # Application launcher
]
```

### Pattern: Exclude Arch Desktop-Specific

```toml
["arch.desktop"]
paths = [
  "config/xmonad/",          # Window manager (needs both Arch and desktop)
  "config/dunst/",           # Notification daemon
  "xinitrc",                 # X11 initialization
]
```

## Relationship with Configuration Processing

**Important Distinction:**

1. **Sparse Checkout** (manifest.toml with OR logic)
   - Controls which files exist in working directory
   - Uses OR logic: `["arch.desktop"]` = exclude if arch OR desktop excluded

2. **Configuration Processing** (packages.toml, symlinks.toml with AND logic)
   - Controls which items are installed/processed
   - Uses AND logic: `["arch.desktop"]` = process only if arch AND desktop active

**Example:**
```toml
# manifest.toml (OR logic)
["arch.desktop"]
paths = ["config/xmonad/"]  # Exclude if EITHER arch OR desktop excluded

# packages.toml (AND logic)
["arch.desktop"]
packages = ["xmonad"]       # Install only if BOTH arch AND desktop active
```

This ensures files are available when their corresponding packages are installed.

## Troubleshooting

### Files Not Being Excluded

1. **Check manifest syntax**: Ensure paths are relative to `symlinks/`
2. **Check directory trailing slash**: Directories must end with `/`
3. **Verify profile mapping**: Check `conf/profiles.toml` for correct excludes
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

1. **Always use OR logic** for manifest.toml sections (unlike other config files)
2. **Include trailing slash** for directories in manifest.toml
3. **Use quoted dotted keys** for multi-category sections: `["arch.desktop"]`
4. **Paths are relative** to `symlinks/` directory (prefix not included in manifest)
5. **Base files don't need listing** - only list files that should be excluded
6. **Test across profiles** after adding new manifest entries
7. **Use multi-category sections** `["arch.desktop"]` when file should be excluded if ANY category is excluded
8. **Verify auto-detection** overrides - system enforces OS compatibility automatically
9. **Clean working directory** required before applying sparse checkout (uncommitted changes cause errors)
10. **Document category choices** when adding new files to manifest

## Related Skills and Documentation

- **`profile-system`** skill - Understanding profiles and selection
- **`toml-configuration`** skill - TOML file format and deserialization
- **`docs/PROFILES.md`** - User-facing profile documentation
- **`docs/ARCHITECTURE.md`** - System architecture overview

## Key Files

- **`conf/manifest.toml`** - File-to-category mappings
- **`conf/profiles.toml`** - Profile-to-exclude-category mappings
- **`cli/src/tasks/sparse_checkout.rs`** - Sparse checkout task implementation
- **`.git/info/sparse-checkout`** - Git's sparse checkout configuration (generated)
