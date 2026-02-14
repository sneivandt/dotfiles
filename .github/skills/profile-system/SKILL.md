---
name: profile-system
description: >
  Understanding the profile-based configuration system in the dotfiles project.
  Use when working with profiles, sparse checkout, or multi-environment support.
metadata:
  author: sneivandt
  version: "1.0"
---

# Profile System

This skill explains how the profile-based configuration system works in the dotfiles project.

## Overview

Profiles control which files are checked out and which configuration sections are processed. This allows a single repository to support multiple environments (Linux headless, Linux desktop, Windows) while only checking out relevant files.

## How Profiles Work

1. **Sparse Checkout**: Git sparse checkout excludes files based on categories defined in `conf/manifest.ini`. This reduces disk usage and clutter by only checking out files relevant to the selected profile.
2. **Section Filtering**: Configuration files use section headers that match profiles to determine which items to process
3. **Automatic Installation**: All components defined in active profile sections are automatically installed
4. **Persistence**: Selected profile is saved in `.git/config` for automatic reuse on subsequent runs

## Available Profiles

- **`base`**: Minimal core shell configuration (excludes OS-specific and desktop files)
- **`arch`**: Arch Linux headless (includes Arch packages, excludes desktop)
- **`arch-desktop`**: Arch Linux desktop (includes desktop tools, window manager, fonts)
- **`desktop`**: Generic Linux desktop (includes desktop tools like VS Code without OS-specific packages)
- **`windows`**: Windows environment (PowerShell, registry settings)

## Auto-Detection Overrides

**IMPORTANT**: System detection always takes precedence over profile configuration to prevent incompatible operations:
- **Non-Arch Linux systems**: Always exclude `arch` category regardless of selected profile
- **Linux systems**: Always exclude `windows` category regardless of selected profile

These overrides ensure compatibility even if an incompatible profile is manually selected.

## Profile Selection Priority

When running `dotfiles.sh`, the profile is determined in this order:
1. **Explicit CLI argument**: `--profile arch-desktop` (highest priority)
2. **Persisted profile**: Reads from `.git/config` (`dotfiles.profile` key)
3. **Interactive prompt**: If neither exists, prompts user to select from available profiles

Example usage:
```bash
# First time - interactive selection
./dotfiles.sh -I
# Prompts: "Select profile (1-4): "
# Selection is saved to .git/config

# Subsequent runs - uses saved profile
./dotfiles.sh -I
# No prompt, uses persisted profile

# Override saved profile
./dotfiles.sh -I --profile base
# Uses 'base' and updates saved profile
```

## Profile Persistence Implementation

Profile persistence uses git config:
- **Save**: `git config --local dotfiles.profile <profile_name>`
- **Read**: `git config --local --get dotfiles.profile`
- **Location**: `.git/config` (not committed, local to repository clone)

See `src/linux/utils.sh` for implementation:
- `get_persisted_profile()`: Reads saved profile
- `persist_profile()`: Saves profile
- `prompt_profile_selection()`: Interactive selection UI
- `list_available_profiles()`: Reads from `conf/profiles.ini`

## Adding New Profiles

1. Add profile definition to `conf/profiles.ini`:
   ```ini
   [my-profile]
   include=
   exclude=windows,desktop
   ```
2. Use with `--profile my-profile` or select interactively

## Adding Configuration Items

When adding packages, units, or other configuration:
1. Add to appropriate INI file under the correct section using comma-separated categories
   - Single category: `[arch]` for Arch-only items
   - Multiple categories: `[arch,desktop]` for items requiring BOTH Arch AND desktop
2. Item will be automatically processed when ALL required categories are active
3. No flags needed - profile determines what gets installed

## Section Naming Convention

**Important Distinction**:
- **Profile names** (in `profiles.ini` only): Use hyphens like `[arch-desktop]`
- **Section names** (all other .ini files): Use comma-separated categories like `[arch,desktop]`
  - Comma-separated sections mean ALL categories must be active (logical AND)
  - Example: `[arch,desktop]` is processed only when both `arch` and `desktop` are not excluded

## Profile Filtering in Code

### Linux (Shell)
Use `should_include_profile_tag()` to check if a section should be processed:
```sh
if should_include_profile_tag "$section"; then
  # Process this section
fi
```

### Windows (PowerShell)
Use `Test-ShouldIncludeSection` to check if a section should be processed:
```powershell
if (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories) {
  # Process this section
}
```
