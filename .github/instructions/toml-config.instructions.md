---
description: >
  TOML configuration conventions for dotfiles. Use when creating or modifying
  config files in conf/: section naming, category tags, item formats.
applyTo: "conf/**/*.toml"
---

# TOML Configuration Conventions

## Section Names

Section names are **hyphen-separated category tags** with AND logic:

```toml
[base]                  # Matches 'base' profile
[arch-desktop]          # Matches when BOTH 'arch' AND 'desktop' are active
[linux]                 # Matches any Linux system
```

Valid categories: `base`, `desktop` (profile); `linux`, `windows`, `arch` (platform, auto-detected).

Do **not** use dots (creates nested TOML tables) or commas in section names.

## Item Formats

Each file has its own key (see table below) — values are simple strings or structured objects:

```toml
[base]
packages = [
  "git",
  { name = "paru", aur = true },
]
```

Always include a trailing comma after the last array element.

## Files and Their Structure

| File | Key field | Notes |
|---|---|---|
| `packages.toml` | `packages` | String or `{ name, aur }` |
| `symlinks.toml` | `symlinks` | `{ src, dest }` |
| `systemd-units.toml` | `units` | `{ name, type }` |
| `vscode-extensions.toml` | `extensions` | String (publisher.name) |
| `git-config.toml` | `entries` | `{ key, value }` |
| `chmod.toml` | `entries` | `{ path, mode }` |
| `registry.toml` | `entries` | `{ path, name, type, value }` (Windows) |

See the `toml-configuration` skill for the full config loader pattern and Rust deserialization.
