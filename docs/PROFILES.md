# Profiles and Categories

Profiles select the role of a machine. Categories describe the layers of
configuration that apply. The CLI combines a selected role profile with
automatically detected platform categories.

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

These categories are detected; users do not select them as profiles.

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

This section applies only when both `arch` and `desktop` are active. Ordering
does not turn it into a hierarchy, and matching is not OR-based.

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
the checkout. `conf\manifest.toml` maps category exclusions to paths under
`symlinks\`.

When switching from `desktop` to `base`, desktop paths may be removed from the
sparse checkout. Before applying those exclusions, the CLI materializes managed
home symlinks whose sources would disappear. This preserves usable user files
rather than leaving broken links.

Preview profile changes:

```bash
dotfiles install --profile base --dry-run --verbose
```

## Adding a profile or category

1. Add the role definition to `conf\profiles.toml`.
2. Add category sections to relevant configuration files.
3. Add matching sparse-checkout coverage to `conf\manifest.toml` for every
   non-`base` symlink section.
4. Run `dotfiles test`.
5. Preview both inclusion and exclusion transitions with `--dry-run`.

Prefer a small vocabulary of orthogonal categories. A new profile should
compose categories rather than duplicate large configuration lists.
