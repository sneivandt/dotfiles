# Configuration Reference

The Rust CLI treats `conf\` as declarative desired state. Configuration is
loaded before task construction, filtered by active categories, validated, and
exposed to tasks through shared handles.

## Files

| File | Shape | Consumer |
|---|---|---|
| `profiles.toml` | Named role profiles with include/exclude categories | Profile resolver |
| `manifest.toml` | Category sections containing sparse-checkout paths | Sparse checkout |
| `symlinks.toml` | Category sections containing home-relative source paths | Symlink tasks |
| `packages.toml` | Category sections containing package strings or AUR records | Package tasks |
| `git-config.toml` | Category sections containing key/value settings | Git configuration |
| `copilot.toml` | Category sections containing dot-path JSON settings | Copilot configuration |
| `chmod.toml` | Category sections containing mode/path records | Unix permissions |
| `registry.toml` | Named registry records with `path` and `values` | Windows registry |
| `systemd-units.toml` | Category sections containing user unit names | systemd configuration |
| `vscode-extensions.toml` | Category sections containing extension identifiers | VS Code extensions |

An overlay may additionally provide `conf\scripts.toml`. The main repository
does not load scripts from that filename.

## Category sections

Most files group records under category names:

```toml
[base]
symlinks = ["config/git/config"]

[windows]
symlinks = ["Documents/PowerShell/Microsoft.PowerShell_profile.ps1"]

[arch-desktop]
symlinks = ["config/hypr/hyprland.conf"]
```

A hyphenated section uses **AND semantics**. `[arch-desktop]` is active only
when both `arch` and `desktop` are active. It does not mean either category.

`base` is always active. Platform categories are detected by the CLI; role
categories come from the selected profile. See [Profiles](PROFILES.md).

## Profiles

`profiles.toml` maps a selectable role to category changes:

```toml
[base]
description = "Core shell environment, no desktop GUI"
include = []
exclude = ["desktop"]

[desktop]
description = "Full desktop/workstation setup with GUI tools"
include = ["desktop"]
exclude = []
```

The selected role is combined with detected `linux`, `windows`, and `arch`
categories. Profile names and category names are related but distinct: a
profile controls a set of categories.

## Symlinks

`symlinks.toml` entries are paths relative to `symlinks\`. Their home target is
the same path prefixed with a dot where appropriate:

```toml
[base]
symlinks = [
  "config/git/config",
  "ssh/config",
]
```

Source globs are supported, for example `apm/plugins/*`. Each resolved source
becomes an independently managed link. Overlay symlinks resolve from the
overlay's own `symlinks\` tree, not the main repository.

Every non-`base` symlink category must have an exact section in
`manifest.toml`, and every manifest section must exist in `symlinks.toml`.
`dotfiles test` verifies this section synchronization. The `config_drift`
integration test separately verifies source-path coverage, compatible subset
sections, and that manifest paths exist.

## Sparse-checkout manifest

`manifest.toml` lists paths relative to `symlinks\` that may be excluded:

```toml
[arch]
paths = [
  "apm/config/arch.yml",
  "config/pacman.conf",
]
```

Directory paths should end in `/`. A manifest section can cover a more specific
symlink section when its category tags are a subset; for example, `[desktop]`
may cover a source linked from `[windows-desktop]`.

The manifest is filtered from the selected profile's **excluded** categories.
Unlike normal desired-state sections, it is not appended from overlays.

## Packages

Package entries are strings unless the package comes from the AUR:

```toml
[arch]
packages = [
  "git",
  "ripgrep",
  { name = "apm-bin", aur = true },
]

[windows]
packages = [
  "Git.Git",
  "Microsoft.PowerShell",
]
```

Arch regular packages use pacman; entries marked `aur = true` are separated for
the AUR task. Windows identifiers are passed to winget.

## Git settings

```toml
[windows]
settings = [
  { key = "core.autocrlf", value = "false" },
  { key = "core.longpaths", value = "true" },
]
```

The CLI applies these settings to global Git configuration. Keep platform-only
behavior in platform category sections.

## Copilot settings

```toml
[base]
settings = [
  { key = "model", value = "gpt-5.6-sol" },
  { key = "footer.showDirectory", value = true },
]
```

Keys are dot-separated paths into `~\.copilot\settings.json`. Only declared
keys are managed. Sibling properties and volatile Copilot CLI state remain
untouched.

## File permissions

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/zsh" },
]
```

Paths are relative to the home directory and modes are Unix octal strings. For
directory trees, traversal access is preserved while ordinary files do not
inherit execute bits unless explicitly targeted.

## Registry

Registry records are not category arrays; each named record declares one path
and a values table:

```toml
[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

String, decimal, and hexadecimal TOML values are supported by the loader. Keep
values under current-user paths unless the implementation explicitly supports
another scope.

## systemd units

```toml
[linux]
units = ["clean-home-tmp.timer"]

[arch-desktop]
units = ["waybar.service"]
```

These are user units. Their unit files are normally delivered through managed
symlinks before the systemd task enables and starts them.

## VS Code extensions

```toml
[desktop]
extensions = [
  "rust-lang.rust-analyzer",
  "tamasfe.even-better-toml",
]
```

Use complete `<publisher>.<extension>` identifiers. The task installs missing
extensions through an available VS Code CLI.

## Overlays

`--overlay <PATH>` adds a second repository. For ordinary configuration, active
overlay entries are **appended** to active main entries; they do not replace
records with the same logical name. This applies to packages, symlinks, Git,
Copilot, permissions, registry records, systemd units, and VS Code extensions.

Important boundaries:

- Missing overlay configuration files are treated as empty.
- Main configuration remains required where validation says it is required.
- Symlink entries retain the repository they came from.
- `manifest.toml` is main-repository-only.
- `scripts.toml` is overlay-only.

Validate combined state explicitly:

```bash
dotfiles test --overlay C:\path\to\private-dotfiles
dotfiles install --overlay C:\path\to\private-dotfiles --dry-run
```

## Overlay scripts

An overlay's `conf\scripts.toml` defines convention-based script tasks. Each
entry has a unique task name and a path relative to the overlay:

```toml
[base]
scripts = [
  {
    name = "Configure private workstation",
    path = "scripts/configure-workstation.ps1",
    description = "Converge private workstation settings"
  },
]
```

The script resource supports four execution intents:

- normal apply
- current-state check through `--check`
- preview through `--dryrun`
- removal through `--remove`

Install and update invoke check, apply, or preview as appropriate. The resource
supports removal, but dynamic scripts are not registered in the current
uninstall catalog, so `dotfiles uninstall` does not invoke `--remove`.

Scripts should be idempotent, return nonzero on failure, and avoid emitting
secrets. Dry-run safety is cooperative: the engine supplies `--dryrun` but
cannot stop an opaque script from mutating state. Every active entry becomes a
normal dynamic task after the reload discovery boundary and can be selected by
its name.

## APM configuration

APM's source fragments are YAML files under `symlinks\apm\config\`, not a TOML
file in `conf\`. The active profile determines which fragments are present in
the sparse checkout and linked configuration. See [APM](APM.md).

## Loading and reload behavior

At command start, the loader:

1. Resolves the active profile and categories.
2. Parses main configuration.
3. Parses existing overlay files.
4. Appends active overlay records.
5. Runs section and aggregate validation.
6. Stores values in shared handles used by tasks.

If **Update repository** changes tracked content, **Reload configuration**
repeats the load and updates those handles. Later tasks therefore observe the
new configuration in the same command invocation.

## Validation

Run:

```bash
dotfiles test
```

The command catches syntax errors, missing required files, nonexistent symlink
sources, manifest drift, and available APM/script analyzer failures. Tests also
include a Rust integration test dedicated to configuration drift.
