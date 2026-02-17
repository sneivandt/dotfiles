---
name: profile-system
description: >
  Understanding the profile-based configuration system in the dotfiles project.
  Use when working with profiles, sparse checkout, or multi-environment support.
metadata:
  author: sneivandt
  version: "2.0"
---

# Profile System

Profiles control which files are checked out and which config sections are processed. Profile resolution is in `cli/src/config/profiles.rs`.

## Available Profiles

- **`base`**: Minimal core shell config (excludes arch, desktop, windows)
- **`arch`**: Arch Linux headless (excludes desktop, windows)
- **`arch-desktop`**: Arch Linux desktop (excludes windows)
- **`desktop`**: Generic Linux desktop (excludes arch, windows)
- **`windows`**: Windows environment (excludes arch)

## How Profiles Work

1. **Profile resolution**: `profiles::resolve()` computes `active_categories` and `excluded_categories`
2. **Platform overrides**: `Platform::excludes_category()` auto-excludes incompatible categories (e.g., `arch` on non-Arch, `windows` on Linux)
3. **Config filtering**: `ini::filter_sections_and()` includes sections where ALL categories are active
4. **Sparse checkout**: `manifest.ini` uses OR-exclude logic to filter repository files

## Profile Selection Priority

```
CLI `-p` (`--profile`) > git config dotfiles.profile > interactive prompt
```

Implemented in `profiles::resolve_from_args()`:
```rust
pub fn resolve_from_args(cli_profile: Option<&str>, root: &Path, platform: &Platform) -> Result<Profile> {
    let name = if let Some(name) = cli_profile { name.to_string() }
    else if let Some(name) = read_persisted(root) { name }
    else { prompt_interactive(platform)? };
    // ...
}
```

The selected profile is persisted to `.git/config` via `git config --local dotfiles.profile`.

## Profile Data Structure

```rust
pub struct Profile {
    pub name: String,
    pub active_categories: Vec<String>,   // e.g., ["base", "arch", "desktop"]
    pub excluded_categories: Vec<String>, // e.g., ["windows"]
}
```

`active_categories` always includes `"base"`. Platform overrides ensure incompatible categories are excluded regardless of profile definition.

## Section Naming Convention

- **Profile names** (in `profiles.ini`): Use hyphens — `[arch-desktop]`
- **Config sections** (all other INI files): Comma-separated — `[arch,desktop]`
  - AND logic: all categories must be in `active_categories`

## Adding a New Profile

Add to `conf/profiles.ini`:
```ini
[my-profile]
include=mycategory
exclude=windows,desktop
```

Add the name to `PROFILE_NAMES` constant in `cli/src/config/profiles.rs`.

## Usage

```bash
./dotfiles.sh install -p arch-desktop -d
./dotfiles.sh install -p base
```

## Rules

- Platform detection always overrides profile config for safety
- Profile names use hyphens; config sections use commas
- `active_categories` always contains `"base"`
- Use `filter_sections_and()` for config filtering (AND logic)
- Use `filter_sections_or_exclude()` for manifest filtering (OR logic)
