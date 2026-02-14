---
name: ini-configuration
description: >
  Guide for working with INI configuration files in the dotfiles project.
  Use when creating, modifying, or parsing INI files in conf/ directory.
metadata:
  author: sneivandt
  version: "1.0"
---

# INI Configuration Guide

This skill provides guidance for working with INI configuration files in the dotfiles project.

## Format Overview

All configuration files in `conf/` use standard INI format with section headers:

### Standard List Format
Most INI files use simple list format (one item per line):
```ini
# Comments start with #
[section-name]
entry-one
entry-two

[another-section]
more-entries
```

### Section Naming Convention

**Important Distinction**:
- **Profile names** (in `profiles.ini` only): Use hyphens like `[arch-desktop]`
- **Section names** (all other .ini files): Use comma-separated categories like `[arch,desktop]`
  - Comma-separated sections mean ALL categories must be active (logical AND)
  - Example: `[arch,desktop]` is processed only when both `arch` and `desktop` are not excluded

### Special Case: Registry Configuration

**Exception**: `conf/registry.ini` is the ONLY config file using `key = value` format:
```ini
[HKCU:\Software\Example]
SettingName = SettingValue
AnotherSetting = AnotherValue
```
Section headers are registry paths, and assignments are registry key/value pairs. **Note**: Registry configuration does not use profile filtering since it is Windows-only by nature. All registry settings are applied when running on Windows.

## Parsing INI Files

### Linux (Shell)

Use `read_ini_section()` helper from `src/linux/utils.sh`:
```sh
read_ini_section "$DIR/conf/packages.ini" "arch" | while IFS='' read -r package
do
  # Process package
done
```

**Behavior**: The helper automatically skips empty lines and comment lines (starting with `#`). Empty sections return no output, so the loop body won't execute. This is safe and expected behavior.

### Windows (PowerShell)

Use `Read-IniSection` helper from `src/windows/Profile.psm1`:
```powershell
$fonts = Read-IniSection -FilePath $configFile -SectionName "fonts"
```

**Behavior**: The helper automatically skips empty lines and comment lines (starting with `#`). Empty sections return an empty array, so iterating over the result is safe.

## Profile Filtering

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

## Configuration Files

- **`profiles.ini`**: Profile definitions with include/exclude categories
- **`manifest.ini`**: Maps files to categories for sparse checkout exclusion
- **`symlinks.ini`**: Symlink mappings organized by category sections
- **`packages.ini`**: System packages organized by category sections
- **`units.ini`**: Systemd user units organized by category sections
- **`chmod.ini`**: File permissions organized by category sections
- **`fonts.ini`**: Font families to check/install
- **`vscode-extensions.ini`**: VS Code extensions in `[extensions]` section
- **`registry.ini`**: Windows registry settings with registry paths as sections
- **`copilot-skills.ini`**: GitHub Copilot CLI skill URLs organized by profile

## Processing Multi-Section Configs

When processing configuration files with multiple profile-based sections:

```sh
# Get all sections from config file
sections="$(grep -E '^\[.+\]$' "$DIR"/conf/file.ini | tr -d '[]')"

# Process each section that matches the profile
for section in $sections; do
  if ! should_include_profile_tag "$section"; then
    log_verbose "Skipping section [$section]: profile not included"
    continue
  fi

  # Process entries in this section
  read_ini_section "$DIR"/conf/file.ini "$section" | while IFS='' read -r item; do
    # Process item
  done
done
```

## Rules

- Empty lines and `#` comments are ignored
- Section names match profile names or categories
- Process only sections that match the active profile
- Always use helper functions for parsing
- Never manually parse INI files
