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

- **`base`**: Minimal core config without desktop apps (excludes desktop)
- **`desktop`**: Full config with desktop apps (includes desktop)

Platform categories (`linux`, `windows`, `arch`) are auto-detected — users only choose between `base` and `desktop`.

## How Profiles Work

1. **Profile resolution**: `profiles::resolve()` computes `active_categories` and `excluded_categories`
2. **Platform auto-detection**: `Platform::excludes_category()` auto-adds/excludes platform categories (`linux`, `windows`, `arch`)
3. **Always-active**: `base` is always in `active_categories`
4. **Config filtering**: `toml_loader::filter_sections(&sections, active, MatchMode::All)` includes sections where ALL categories are active
5. **Sparse checkout**: `manifest.toml` uses OR-exclude logic to filter repository files

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

The selected profile is persisted to `.git/config` by writing to it directly via
file I/O (`profiles::persist()` uses a custom `set_git_config_value()` helper).
Running `git config --local dotfiles.profile <name>` manually produces the same
result.

## Profile Data Structure

```rust
pub struct Profile {
    pub name: String,
    pub active_categories: Vec<String>,   // e.g., ["base", "arch", "desktop"]
    pub excluded_categories: Vec<String>, // e.g., ["windows"]
}
```

`active_categories` always includes `"base"`. Platform categories (`linux`, `windows`, `arch`) are auto-added or auto-excluded based on `Platform::excludes_category()`. Users only choose the role (`base` or `desktop`).

## Section Naming Convention

- **Profile names** (in `profiles.toml`): e.g., `[base]`, `[desktop]`
- **Config sections** (all other TOML files): Hyphen-separated — `[arch-desktop]`
  - AND logic: all categories must be in `active_categories`
- **Platform categories** (`linux`, `windows`, `arch`): Auto-detected, use in config sections for platform-specific items (e.g., `[linux]` for chmod entries)

## Adding a New Profile

Add to `conf/profiles.toml`:
```toml
[my-profile]
include = ["mycategory"]
exclude = []
```

Add the name to `PROFILE_NAMES` constant in `cli/src/config/profiles.rs`.

## Usage

```bash
./dotfiles.sh install -p desktop -d
./dotfiles.sh install -p base
```

## Rules

- Platform detection always overrides profile config for safety
- Profile names are `base` or `desktop`; config section categories use commas
- `active_categories` always contains `"base"` plus auto-detected platform categories
- Use `filter_sections(&sections, active, MatchMode::All)` for config filtering (AND logic)
- Use `filter_sections(&sections, excluded, MatchMode::Any)` for manifest filtering (OR logic)
