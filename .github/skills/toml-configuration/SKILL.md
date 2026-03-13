---
name: toml-configuration
description: >
  Guide for working with TOML configuration files in the dotfiles project.
  Use when creating, modifying, or parsing TOML files in conf/ directory.
metadata:
  author: sneivandt
  version: "3.0"
---

# TOML Configuration Guide

All configuration in `conf/` uses TOML format, deserialized via Serde.

## Array Format (most files)

```toml
# Comments start with #
[section-name]
items = [
  "entry-one",
  "entry-two",
]
```

Deserialized using `#[derive(Deserialize)]` structs with wrapper types containing `Vec<T>`.

## Structured Metadata Format

Many config files support both simple strings and structured metadata:

```toml
# packages.toml — Simple string or object with metadata
[base]
packages = [
  "git",
  "vim",
  { name = "yay", aur = true },
]
```

Deserialized using `#[serde(untagged)] enum` for polymorphic types.

## Multi-Category Sections

Use **hyphen-separated** table names for multi-category sections (AND logic):

```toml
# Hyphen-separated categories
[arch-desktop]
packages = ["rofi", "picom"]
```

The TOML loader splits section names on `-` to extract category tags.
Do **not** use dots or commas — dots create nested TOML tables and commas are
not valid in unquoted table names.

## Section Filtering

```rust
use crate::config::toml_loader;

// Load and filter by category in one step
let items = toml_loader::filter_by_categories(parsed_sections, active_categories);
```

`filter_by_categories()` always uses AND logic: a section matches only when all
of its category tags are present in the category slice passed by the caller.
Most config modules pass `active_categories`; `manifest.rs` passes
`excluded_categories`.

## Config Loader Pattern

Each module in `cli/src/config/` follows:

```rust
use serde::Deserialize;
use crate::config::toml_loader;

#[derive(Deserialize)]
struct MySection {
    items: Vec<MyType>,
}

pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<MyType>> {
    // Load TOML file into HashMap
    let config: HashMap<String, MySection> = toml_loader::load_config(path)?;

    // Convert to (category, Vec<items>) pairs
    let sections: Vec<(String, Vec<MyType>)> = config
        .into_iter()
        .map(|(cat, section)| (cat, section.items))
        .collect();

    // Filter by active categories and flatten
    Ok(toml_loader::filter_by_categories(sections, active_categories))
}
```

## Configuration Files

| File | Format | Notes |
|------|--------|-------|
| `profiles.toml` | table | Profile definitions with `include`/`exclude` arrays |
| `manifest.toml` | arrays | Sparse checkout exclusions using `excluded_categories` with the same AND logic |
| `symlinks.toml` | arrays | Profile-filtered symlink paths |
| `packages.toml` | arrays | Simple strings or `{ name, aur }` objects |
| `systemd-units.toml` | arrays | Systemd unit names |
| `chmod.toml` | arrays | Objects with `mode` and `path` fields |
| `vscode-extensions.toml` | arrays | Extension IDs |
| `registry.toml` | tables | `path` field + `values` table for registry keys |
| `copilot-plugins.toml` | arrays | Plugin entries |

## TOML Format Examples

### Simple Array (systemd-units.toml)

```toml
[base]
units = ["sshd", "docker"]

[arch-desktop]
units = ["gdm"]
```

### Structured Objects (chmod.toml)

```toml
[base]
entries = [
  { mode = "0600", path = "~/.ssh/config" },
  { mode = "0700", path = "~/.gnupg" },
]
```

### Polymorphic Types (packages.toml)

```toml
[base]
packages = [
  "git",
  { name = "yay", aur = true },
]
```

### Nested Tables (registry.toml)

```toml
[explorer]
path = "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced"

[explorer.values]
HideFileExt = 0x00000000
ShowHidden = 0x00000001
```

## Rules

- Use **hyphen-separated** table names for multi-category sections: `[arch-desktop]`
- All config files use TOML arrays or tables — no custom parsing
- Categories are extracted from table names (lowercased, split on `-`)
- Serde handles deserialization with strong typing
- Use `#[serde(untagged)]` for polymorphic enums (string vs object)
- Always validate TOML syntax — malformed files cause deserialization errors
- Config loaders are called in `Config::load()` (`cli/src/config/mod.rs`)
