# Configuration reference

The Rust CLI loads desired state from `conf/` before constructing tasks. It
filters records by active category, validates them, and exposes them through
shared handles.

## Files

| File | Shape | Consumer |
|---|---|---|
| `profiles.toml` | Named role profiles with include/exclude categories | Profile resolver |
| `symlinks.toml` | Category sections containing home-relative source paths | Symlink tasks |
| `packages.toml` | Category sections containing package strings or AUR records | Package tasks |
| `git-config.toml` | Category sections containing key/value settings | Git configuration |
| `agent-settings.toml` | Targeted dot-path JSON/TOML settings | Agent harness configuration |
| `chmod.toml` | Category sections containing mode/path records | Unix permissions |
| `registry.toml` | Named registry records with `path` and `values` | Windows registry |
| `systemd-units.toml` | Category sections containing user or system unit records | systemd configuration |
| `vscode-extensions.toml` | Category sections containing extension identifiers | VS Code extensions |

An overlay may also provide `conf/scripts.toml`. The main repository does not
load scripts from that file.

### Conflicting desired state

Active Git settings and Windows registry entries must declare only one desired
value per target, including entries appended from an overlay. Conflicting
declarations fail configuration loading before any task runs, even with `--only`
or `--dry-run`. The error reports both source files and their section/entry
locations using `git.conflicting-values` or `registry.conflicting-values`.
Overlay append order is not an override mechanism.

Identical declarations remain valid. Git section and variable names are
case-insensitive, subsection names are case-sensitive, and setting values are
compared literally. Registry key paths and value names are case-insensitive;
both the native value type and data must agree. Equivalent DWORD forms such as
`14` and `"0x0E"` agree, but DWORD `14` and string `"14"` conflict. Inactive Git
categories and registry entries on non-Windows platforms do not participate.

## Category sections

Most files group records under category names:

```toml
[base]
symlinks = ["config/git/config"]

[windows]
symlinks = [
  { source = "config/powershell/Microsoft.PowerShell_profile.ps1", target = "Documents/PowerShell/Microsoft.PowerShell_profile.ps1" },
]

[arch-desktop]
symlinks = ["config/hypr/hyprland.lua"]
```

A hyphenated section uses AND semantics. `[arch-desktop]` is active only
when both `arch` and `desktop` are active. It does not mean either category.
Category tags must be built in or declared by a profile; misspelled tags are
configuration errors.

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

`symlinks.toml` entries are paths relative to `symlinks/`. Their home target is
the same path prefixed with a dot, so `config/git/config` links to
`~/.config/git/config`:

```toml
[base]
symlinks = [
  "config/git/config",
  "ssh/config",
]
```

To override the dot-prefixed default, use a table with an explicit
home-relative `target`. Windows paths such as `AppData/` use this form because
they have no leading dot:

```toml
[windows]
symlinks = [
  { source = "config/nuget/nuget.config", target = "AppData/Roaming/NuGet/nuget.config" },
]
```

The same canonical source may appear more than once if each entry has a
different target. Multiple applications can then share one configuration
without forwarding files or symlink chains inside the repository. The loader
rejects a source that resolves outside its owning `symlinks/` tree.

Overlay symlinks resolve from the overlay's own `symlinks/` tree, not the main
repository.

### Glob patterns

A source path may use `*` to link every entry of a directory, so a new file
does not have to be added to `symlinks.toml` by hand:

```toml
[base]
symlinks = [
  "apm/plugins/*",
]
```

Rules:

- `*` matches exactly **one complete path segment**. Partial-segment patterns
  such as `plugins/*.yml` or `plugins/apm-*` are rejected, as is the recursive
  wildcard `**`. Both produce a configuration error rather than matching
  nothing.
- Every directory entry matches, files and directories alike, including
  dot-prefixed ones. Each resolved source becomes an independently managed
  link.
- Matching does not descend through a symlinked directory, so expansion cannot
  escape the `symlinks/` tree.
- A glob must match at least one entry. An empty match is a configuration
  error, which catches renamed or deleted directories instead of silently
  managing nothing.
- Matches are sorted by path, so run output and dry-run previews are stable.

A `target` may contain `*` only when its `source` does, and both must use the
same number of wildcards. The *n*th `*` in the target is replaced with the
segment captured by the *n*th `*` in the source:

```toml
[base]
symlinks = [
  { source = "skills/*", target = ".copilot/skills/*" },
]
```

Globs expand during configuration loading, after category filtering and before
validation. Later stages see only concrete paths. Expanded targets use the
normal conflict checks. Duplicate targets and targets nested under another
managed target are errors.

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

## Agent harness settings

```toml
[base]
settings = [
  { target = "copilot", key = "model", value = "gpt-5.6-sol" },
  { target = "codex", key = "model", value = "gpt-5.6-sol" },
  { target = "codex", key = "model_reasoning_effort", value = "high" },
]
```

Supported targets are `copilot` (`~/.copilot/settings.json`) and `codex`
(`~/.codex/config.toml`). Keys are dot-separated paths, and only declared keys
are managed; siblings and volatile harness-owned state remain untouched.

Codex uses the same user configuration for its CLI, IDE extension, and agent
inside the ChatGPT desktop app. ChatGPT Work chats do not read local Codex
configuration, and app-only appearance, notification, and keyboard preferences
remain managed through the desktop app.

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

The loader accepts string, decimal, and hexadecimal TOML values. Keep values
under current-user paths unless the implementation explicitly supports another
scope.

## systemd units

```toml
[linux]
units = ["clean-home-tmp.timer"]

[arch-desktop]
units = [
  { name = "NetworkManager.service", scope = "system" },
  { name = "dhcpcd.service", scope = "system", enabled = false },
  "quickshell.service",
]
```

A bare string uses `user` scope and defaults to `enabled = true`. Use a table
to select `user` or `system` scope or to keep a conflicting unit disabled.
User unit files are normally delivered through managed symlinks before the task
enables and starts them. Changing a system unit uses `sudo`.

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

Overlay rules:

- Missing overlay configuration files are treated as empty.
- Main configuration remains required where validation says it is required.
- Symlink entries retain the repository they came from.
- `scripts.toml` is overlay-only.

When an overlay is active, its resolved path is reported as the final
` · overlay <path>` section of the startup header line.

When `--overlay` points to a linked Git worktree, the CLI asks before using and
persisting that path. The `[y/N]` prompt defaults to no. A non-interactive
invocation rejects the new worktree path. Normal checkouts have a `.git`
directory and do not require confirmation.

Validate combined state using the overlay examples in
[CLI validation](TESTING.md#cli-validation) and
[Dry-run testing](TESTING.md#dry-run-testing).

## Overlay scripts

An overlay's `conf/scripts.toml` defines convention-based script tasks. Each
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

Install invokes check, apply, or preview as appropriate. The resource
supports removal, but dynamic scripts are not registered in the current
uninstall catalog, so `dotfiles uninstall` does not invoke `--remove`.

Scripts must be idempotent, return nonzero on failure, and avoid printing
secrets. Dry-run safety depends on the script. The engine supplies `--dryrun`,
but it cannot prevent a script from changing state. After the reload discovery
boundary, each active entry becomes a dynamic task selectable by name.

## APM configuration

APM's source fragments are YAML files under `symlinks/apm/config/`, not a TOML
file in `conf/`. The active profile determines which fragments are linked into
the home configuration. See [APM](APM.md).

## Loading and reload behavior

At startup, the loader:

1. Resolves the active profile and categories.
2. Parses main configuration.
3. Parses existing overlay files.
4. Appends active overlay records.
5. Runs section and aggregate validation.
6. Stores values in shared handles used by tasks.

If **Dotfiles repository** changes tracked content, the internal **Reload configuration**
repeats the load and updates those handles. Later tasks therefore observe the
new configuration in the same command invocation. The reload runs the same
section and aggregate validation as startup and reports any diagnostics, so a
configuration pulled mid-run is held to the same standard as one present when
the command began.

## Unknown keys are errors

Every main `conf/*.toml` file listed above is required and parsed strictly,
including files for inactive platforms. An unrecognized or misspelled key
aborts loading. The error includes the file, line, column, and accepted keys:

```text
ERROR Invalid TOML syntax in conf/profiles.toml: TOML parse error at line 13, column 1
13 | excludee = ["desktop"]
unknown field `excludee`, expected one of `description`, `include`, `exclude`
```

This applies to section fields such as `symlink` instead of `symlinks`, keys
inside table entries such as `targett` instead of `target`, and profile
definitions. Typos do not fall back to defaults. For example, `excludee` in
`profiles.toml` once produced an empty exclude list, causing the `base` profile
to stop excluding desktop categories.

Section category tags are checked too. Built-in tags are `base`, `desktop`,
`linux`, `windows`, and `arch`; custom tags must appear in a profile's
`include` or `exclude` list before another config file can use them.

Entries that accept either a bare string or a table (symlinks, packages,
systemd units) are also strict in both forms: a value that is neither a string
nor a table is rejected by kind.

## Validation

The validation workflow catches syntax errors, unknown keys, missing required
files, nonexistent symlink sources, and failures from available APM or script
analyzers. A separate Rust integration test checks configuration drift. Commands
and focused coverage are documented in
[Testing](TESTING.md#cli-validation).
