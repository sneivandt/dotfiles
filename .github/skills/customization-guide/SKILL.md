---
name: customization-guide
description: >
  Technical guide for programmatically adding configuration items to the dotfiles project.
  Use when an agent needs to add symlinks, packages, systemd units, VS Code extensions, or Windows registry settings.
metadata:
  author: sneivandt
  version: "2.0"
---

# Customization Guide

Patterns for extending the dotfiles system. Task logic is in `cli/src/tasks/*.rs`; config is loaded from INI files in `conf/` by `cli/src/config/*.rs`.

## Configuration Types

| Type | INI File | Config Module | Task Module |
|------|----------|---------------|-------------|
| Symlinks | `symlinks.ini` | `config::symlinks` | `tasks::symlinks` |
| Packages | `packages.ini` | `config::packages` | `tasks::packages` |
| Systemd | `units.ini` | `config::units` | `tasks::systemd` |
| VS Code | `vscode-extensions.ini` | `config::vscode` | `tasks::vscode` |
| Registry | `registry.ini` | `config::registry` | `tasks::registry` |
| Chmod | `chmod.ini` | `config::chmod` | `tasks::chmod` |
| Fonts | `fonts.ini` | `config::fonts` | `tasks::fonts` |
| Copilot Skills | `copilot-skills.ini` | `config::copilot_skills` | `tasks::copilot_skills` |

All configs are profile-aware (loaded via `config::ini::filter_sections_and`).

## Adding Symlinks

1. Create source in `symlinks/` (no leading dot): `symlinks/config/myapp/config.yml`
2. Add to `conf/symlinks.ini`: `[base]\nconfig/myapp/config.yml`
3. Optionally add to `conf/manifest.ini` for sparse checkout
4. Test: `./dotfiles.sh install -d`

See `symlink-management` skill.

## Adding Packages

```ini
[arch]
my-package
[arch,aur]
my-aur-package-bin
[windows]
Microsoft.PowerShell
```

The config loader tags AUR packages from `[*,aur]` sections. See `package-management` skill.

## Adding Systemd Units

Create unit in `symlinks/config/systemd/user/`, add to `conf/symlinks.ini` and `conf/units.ini`.

## Adding VS Code Extensions

```ini
# conf/vscode-extensions.ini — [extensions] is not profile-filtered
[extensions]
ms-python.python
```

## Adding Registry (Windows)

```ini
# conf/registry.ini — key=value format, not profile-filtered
[HKCU\Software\MyApp]
Setting=REG_SZ:Value
```

## Profile Sections

`[base]` all systems · `[arch]` Arch Linux · `[arch,desktop]` desktop GUI · `[windows]` Windows. See `profile-system` skill.

## Rules

1. Each type uses its dedicated INI file in `conf/`
2. Test with `./dotfiles.sh install -d`
3. One item per line; no leading dots in `symlinks/`
4. Commit source files and INI entries atomically
5. See `ini-configuration` skill for format details
