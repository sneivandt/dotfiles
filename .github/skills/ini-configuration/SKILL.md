---
name: ini-configuration
description: >
  Guide for working with INI configuration files in the dotfiles project.
  Use when creating, modifying, or parsing INI files in conf/ directory.
metadata:
  author: sneivandt
  version: "2.0"
---

# INI Configuration Guide

All configuration in `conf/` uses INI format, parsed by `cli/src/config/ini.rs`.

## List Format (most files)

```ini
# Comments start with #
[section-name]
entry-one
entry-two
```

Parsed by `ini::parse_sections()` → `Vec<Section>` with `categories: Vec<String>` and `items: Vec<String>`.

## Key-Value Format (registry.ini only)

```ini
[HKCU\Software\Example]
SettingName = SettingValue
```

Parsed by `ini::parse_kv_sections()` → `Vec<KvSection>` with `entries: Vec<(String, String)>`.

## Section Naming

- **Profile names** (`profiles.ini`): e.g., `[base]`, `[desktop]`
- **Config sections** (all others): comma-separated — `[arch,desktop]`
  - AND logic: all categories must be active

## Profile Filtering

```rust
// Include sections where ALL categories are active
pub fn filter_sections_and(sections: &[Section], active: &[String]) -> Vec<Section> {
    sections.iter()
        .filter(|s| s.categories.iter().all(|c| active.contains(c)))
        .cloned().collect()
}
// Exclude sections where ANY category is excluded (for manifest)
pub fn filter_sections_or_exclude(sections: &[Section], excluded: &[String]) -> Vec<Section>
```

## Config Loader Pattern

Each type in `cli/src/config/` follows:
```rust
pub fn load(path: &Path, active: &[String]) -> Result<Vec<T>> {
    let sections = ini::parse_sections(path)?;
    let filtered = ini::filter_sections_and(&sections, active);
    // Parse items from filtered sections
}
```

## Configuration Files

| File | Format | Notes |
|------|--------|-------|
| `profiles.ini` | key=value | Profile definitions |
| `manifest.ini` | list | Sparse checkout (OR-exclude) |
| `symlinks.ini` | list | Profile-filtered |
| `packages.ini` | list | `[*,aur]` tags AUR packages |
| `systemd-units.ini` | list | Systemd units |
| `chmod.ini` | list | `<mode> <path>` format |
| `vscode-extensions.ini` | list | `[extensions]` (special) |
| `registry.ini` | key=value | Registry paths as headers |
| `copilot-skills.ini` | list | Skill URLs |

## Rules

- Empty lines and `#` comments are ignored by the parser
- Always use `config::ini` functions — never manually parse
- Categories are lowercased and trimmed automatically
- Items outside sections cause a parse error
- Config loaders are called in `Config::load()` (`cli/src/config/mod.rs`)
