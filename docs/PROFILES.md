# Profiles and categories

A profile selects the machine's role. Categories determine which configuration
sections apply. The CLI combines the selected profile with detected platform
categories.

## Built-in role profiles

| Profile | Includes | Excludes | Intended use |
|---|---|---|---|
| `base` | No optional role category | `desktop` | Servers, WSL, and minimal shell environments |
| `desktop` | `desktop` | Nothing | Workstations with GUI packages and configuration |

`base` configuration is always active regardless of the selected role.

## Automatic categories

| Category | Active when |
|---|---|
| `linux` | The CLI is running on Linux |
| `windows` | The CLI is running on Windows |
| `arch` | The Linux distribution is Arch Linux |

The CLI detects these categories. They are not selectable profiles.

Examples:

| Machine and profile | Active categories | Typical matching sections |
|---|---|---|
| Windows + `base` | `base`, `windows` | `[base]`, `[windows]` |
| Windows + `desktop` | `base`, `windows`, `desktop` | Plus `[desktop]`, `[windows-desktop]` |
| Arch + `base` | `base`, `linux`, `arch` | Plus `[linux]`, `[arch]` |
| Arch + `desktop` | `base`, `linux`, `arch`, `desktop` | Plus `[linux-desktop]`, `[arch-desktop]` |

## Section matching

Section names are split on hyphens and every tag must be active:

```toml
[arch-desktop]
packages = ["waybar"]
```

This section applies only when both `arch` and `desktop` are active. Tag order
does not create a hierarchy, and matching does not use OR semantics.

## Resolution priority

The role profile is resolved in this order:

1. `--profile <name>`
2. `DOTFILES_PROFILE`
3. Repository-local Git config `dotfiles.profile`
4. Interactive selection

An explicitly supplied unknown profile is an error. During interactive
selection, the chosen profile is persisted to repository-local Git config for
future runs.

```bash
dotfiles install --profile desktop
```

```powershell
$env:DOTFILES_PROFILE = "base"
.\dotfiles.ps1 install
```

## Sparse checkout

Profiles affect both desired state and which platform-specific files remain in
the checkout. `conf/manifest.toml` maps category exclusions to paths under
`symlinks/`.

Switching from `desktop` to `base` may remove desktop paths from the sparse
checkout. Before applying the exclusions, the CLI replaces affected managed
home symlinks with their file or directory content. The home paths remain
usable after their sources disappear.

Preview profile changes:

```bash
dotfiles install --profile base --dry-run --verbose
```

## Adding a profile or category

1. Add the role definition to `conf/profiles.toml`.
2. Add category sections to relevant configuration files.
3. Add matching sparse-checkout coverage to `conf/manifest.toml` for every
   non-`base` symlink section.
4. Run `dotfiles test`.
5. Preview both inclusion and exclusion transitions with `--dry-run`.

Keep categories independent and limited in number. Compose a new profile from
categories instead of duplicating large configuration lists.
