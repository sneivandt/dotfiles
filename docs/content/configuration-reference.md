---
title: Configuration reference
description: Declare packages, symlinks, services, and settings in TOML, with category selection and private overlays.
---

# Configuration reference

To change what dotfiles manages, edit the declaration in `conf/`. To change
an application's settings, edit its source under `symlinks/`. Administrator-file
fragments live under `system/`.

**Edit sources, not generated output.** Installed files may be live links into
this checkout, so source edits can affect applications immediately, before
another update. Shell preferences, desktop appearance and keybindings are
covered in [Application settings](application-settings.md).

## Making a configuration change

1. Find the owning file below.
2. Choose the category that should receive the change. A package, its linked
   configuration and its service should have compatible categories.
3. Check that file's path rules. Not every home-relative path has the same
   meaning.
4. Validate the selected configuration and preview the relevant
   [task](task-reference.md). See [Testing](testing.md#cli-validation) for commands and
   [preview limits](testing.md#dry-run-testing).
5. Back up existing targets before applying. Removing a declaration usually
   stops management; it does not undo an earlier change.

Examples show standalone file syntax. When editing an existing file, extend
its section rather than adding a second table with the same name.

## Files

All nine main files are required, even on platforms where their tasks do not
apply. A required file may be empty.

| File | Entry field | Purpose |
|---|---|---|
| [`symlinks.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml) | `symlinks` | Link repository content into home |
| [`packages.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml) | `packages` | Install Arch or Windows packages |
| [`git-config.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml) | `settings` | Set global Git values |
| [`agent-settings.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/agent-settings.toml) | `settings` | Set selected Copilot/Codex preferences |
| [`chmod.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml) | `permissions` | Apply Unix file/directory modes |
| [`registry.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/registry.toml) | `path` and `values` | Set Windows current-user values |
| [`systemd-units.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/systemd-units.toml) | `units` | Enable/start or disable/stop units |
| [`system-files.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) | `files` | Merge fragments into files below `/etc` |
| [`vscode-extensions.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml) | `extensions` | Install editor extensions |

Private overlays may also provide
[`conf/scripts.toml`](private-overlays.md#overlay-scripts). The main repository's
`conf/scripts.toml` is not loaded.

## Category sections

Except for registry records, configuration is grouped by category:

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

The available tags are `base`, `desktop`, `linux`, `windows`, `arch` and `wsl`.
A hyphen means **AND**: `[arch-desktop]` needs both categories.
Unknown or misspelled tags are errors, not new custom categories.

### Profiles

`base` is always active. Selecting the `desktop` profile adds `desktop`;
the CLI supplies platform categories. Profiles describe a machine's role, not
its operating system. See [Profiles](profiles-and-categories.md) for selection precedence,
persistence and platform combinations.

## Path conventions

| Entry | Source/base | Destination |
|---|---|---|
| Symlink `"config/git/config"` | `<repo>/symlinks/config/git/config` | `~/.config/git/config` |
| Symlink with `target = "Documents/file"` | `<repo>/symlinks/<source>` | `~/Documents/file`, exactly as written |
| Permission `path = "ssh/config"` | Home | `~/.ssh/config`, with the leading dot added |
| System file `source = "pacman.conf"` | `<repo>/system/pacman.conf` | `/etc/pacman.conf`, unless `target` is supplied |
| Script `path = "scripts/setup.sh"` | Overlay root | `<overlay>/scripts/setup.sh` |

Here `<repo>` means the repository that **declared the entry**, including an
overlay. Use `/` in portable paths. Home-relative paths are not shell
expressions: write the relative path, not `~/…`.

## Symlinks

A string names a source and derives a dot-prefixed home target:

```toml
[base]
symlinks = ["config/git/config", "ssh/config"]
```

The table form requires both `source` and `target`. It is useful for Windows
locations or for sharing one source across several destinations:

```toml
[windows]
symlinks = [
  { source = "config/nuget/nuget.config", target = "AppData/Roaming/NuGet/nuget.config" },
]

[linux]
symlinks = [
  "vim",
  { source = "vim", target = ".config/nvim" },
]
```

Paths must stay below their respective roots: sources inside the declaring
repository's `symlinks/`, targets inside home. Absolute paths, `..`, and entries
naming the root itself are invalid. `./bashrc` normalizes to the same target
as `bashrc`.

Two active entries within one repository cannot own the same target or
overlapping parent/child targets. An overlay can replace an exact main target,
but cannot turn a parent/child conflict into a valid pair. Target comparison
normalizes separators and `.` components, and ignores case on Windows.
Targets that alias their own source through existing directory links are
rejected.

**Installation does not back up existing files.** A regular file or empty
directory at a managed target can be replaced. A nonempty real directory
fails rather than being recursively deleted. See
[Home symlinks](task-reference.md#home-symlinks) and [Remove tasks](task-reference.md#remove-tasks)
before taking over an existing application's configuration.

### Glob patterns

Use a complete `*` segment to link entries individually:

```toml
[base]
symlinks = ["apm/plugins/*"]
```

This is intentionally smaller than shell glob syntax:

- `*` matches one path segment, including dot-prefixed names.
- Partial patterns such as `*.yml` and recursive `**` are invalid.
- Expansion does not descend through symlinked directories.
- No matches, or a matched filename that is not valid UTF-8, is an error.
- Matches are sorted and checked for target conflicts after category filtering.

An explicit target may contain captures. If it does, source and target must
contain the same number of `*` segments, substituted in order:

```toml
[base]
symlinks = [
  { source = "skills/*", target = ".copilot/skills/*" },
]
```

That example requires entries under `symlinks/skills/`. For this repository's
agent content, prefer its existing [APM deployment](ai-agent-configuration.md) rather
than introducing another deployment route.

## Packages

```toml
[arch]
packages = [
  "git",
  "ripgrep",
  { name = "powershell-dotfiles", aur = true },
]

[windows]
packages = ["Git.Git", "Microsoft.PowerShell"]
```

A string is shorthand for `{ name = "...", aur = false }`. Package names are
provider-specific, not portable aliases. Use Arch names in Arch categories
and winget IDs in Windows categories.

Regular Arch entries use pacman. `aur = true` sends an entry through Paru;
that includes configured [local recipes](package-management.md#local-arch-packages), not just remote
AUR recipes. The host's package-manager repositories and policies, including
`IgnorePkg`, remain in effect.

**When regular Arch packages are missing, installation uses `pacman -Syu`.**
It can upgrade other installed packages too. If every declared package is
already installed, this task does not perform an upgrade merely because the
command is `update`. See [System packages](task-reference.md#system-packages).

## Git settings

```toml
[windows]
settings = [
  { key = "core.longpaths", value = "true" },
]
```

Both fields are strings, including booleans written as `"true"` or `"false"`.
These settings affect **global** Git configuration. Avoid contradicting the
shared files under [`symlinks/config/git/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git).

On Windows, the task also removes global `core.autocrlf` when no active
declaration manages it—even if `settings` is empty. Declare that key explicitly
if you want the task to manage a value instead. This cleanup is separate from
the repository-local Git values used to remember profiles and overlays.

For Git aliases such as `git gr` and `git contributors`, see
[Application settings](application-settings.md#git-shortcuts).

## Agent harness settings

```toml
[base]
settings = [
  { target = "copilot", key = "footer.showBranch", value = true },
  { target = "codex", key = "model_reasoning_effort", value = "medium" },
]
```

| Target | Document |
|---|---|
| `copilot` | `~/.copilot/settings.json` |
| `codex` | `~/.codex/config.toml` |

`target`, `key` and `value` are required. Keys are dot-separated paths; values
use TOML types. Choose the narrowest key that expresses the preference.
Undeclared and harness-owned keys remain untouched.

Existing documents must be valid JSON objects or TOML tables. Malformed
documents fail rather than being replaced with empty ones. Changes are
published atomically and retain existing symlink redirections.

Numbers must be finite, including inside arrays or tables. Overlapping keys
must agree: `footer = { showBranch = true }` conflicts with
`footer.showBranch = false`, but agrees with `footer.showBranch = true`.
Conflicts within either source are rejected before overlay replacement; merged
parent/child keys are checked too.

An overlay replaces an exact, case-sensitive key for the same harness. Other
keys remain managed. Removing a declaration leaves the stored preference in
place. Skills, plugins, hooks and MCP configuration belong to
[AI agent configuration](ai-agent-configuration.md), not this file.

## File permissions

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/zsh" },
]
```

Both fields are required. `mode` is a three- or four-digit octal **string**.
`path` uses the dot-prefixed home convention: `ssh/config` means
`~/.ssh/config`. `./` components normalize; absolute paths, parent traversal
and home itself are invalid.

Missing targets are not created. A directory entry applies recursively:
directories retain traversal access and ordinary files have execute bits
cleared. Give executable files their own entries. Explicit descendant entries
override recursive ancestors regardless of ordering or parallel execution.

Permission changes through managed links can affect the source checkout.

## Registry

Registry sections are labels, **not categories**. Every record applies on
Windows regardless of profile:

```toml
[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

Each record needs `path` and `values`; only `HKCU:\` is supported.
Use TOML literal strings, as above, to avoid escaping backslashes.

| TOML value | Registry representation |
|---|---|
| Integer or boolean | `REG_DWORD` |
| Parseable hexadecimal string, such as `"0x0E"` | `REG_DWORD` |
| Ordinary string, including `"14"` | `REG_SZ` |

Use DWORD values in the unsigned 32-bit range. Other TOML types are serialized
as strings; explicit strings are clearer. DWORD `14` and `"0x0E"` represent
the same desired value; DWORD `14` and string `"14"` do not.

The file is structurally decoded on non-Windows systems too, but its entries
are not applied there. Existing undeclared values are preserved.

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

A string means an enabled **user** unit. A table requires `name` and `scope`;
`scope` is `user` or `system`, and `enabled` defaults to `true`.
Include a supported suffix: `.service`, `.timer`, `.socket`, `.target`, `.path`,
`.mount`, `.automount` or `.swap`.

With a live manager:

- `enabled = true` means persistently enabled and started. Runtime-only
  enablement is not sufficient.
- `enabled = false` means disabled and stopped, including stopping a static
  unit that has no enablement links.
- Disabling is not masking: another unit may start it later.

User unit files usually arrive through symlinks; packages may provide others.
System-scope changes can require sudo. Inside WSL, systemd must already be
running or degraded; changing `wsl.conf` cannot start it in the current session.

Without a user manager, the task prepares per-user enablement links for the
next login rather than starting services. It searches the user's unit directory
first, then `/etc`, `/run`, `/usr/local/lib`, `/usr/lib` and `/lib` systemd user
directories. Managed links keep their installed home paths, so later symlink
materialization does not strand them in the removed checkout.

Offline disable removes matching existing `.wants`/`.requires` links even if
the unit file disappeared or its install directives changed. Offline enablement
honors `WantedBy=`/`RequiredBy=` resets and systemd-style continued assignments.
See [Systemd units](task-reference.md#systemd-units) for task applicability.

## System files

Use structured fragments for administrator-owned files, not home symlinks:

```toml
[linux]
files = [{ source = "codex/requirements.toml", merge = "toml" }]

[arch]
files = [{ source = "pacman.conf", merge = "ini" }]

[arch-desktop]
files = [{ source = "pam.d/login", merge = "pam" }]

[wsl]
files = [{ source = "wsl.conf", merge = "ini" }]
```

`source` and `merge` are required. Sources belong under the declaring
repository's `system/`. The optional `target` defaults to `/etc/<source>` and
must be an absolute path strictly below `/etc`; neither path may contain `..`.
Duplicate active destinations within a repository are errors.

| Strategy | What the fragment manages |
|---|---|
| `toml` | Keys, recursively merging tables |
| `ini` | Assigned keys and bare flags within sections, including Pacman options |
| `pam` | Matching module rules, placed after the final rule in each facility stack |

Unrelated content is preserved. TOML and INI fragments are validated even when
the destination is absent. INI cannot assign different values to the same key
within one section; identical repeated assignments are allowed.

PAM needs an existing service file and facility stack. Rule ownership uses the
facility and actual module field, not a module name appearing in arguments or
an included-stack name. Facility matching ignores case and the optional leading
`-`; the fragment's spelling is retained. Blank lines and full-line comments in
PAM fragments are not inserted.

**These are privileged changes, not a reversible patch.** An overlay replaces
the whole entry for a matching destination, including source and strategy.
Removing a declaration does not restore previous content.

## VS Code extensions

```toml
[desktop]
extensions = ["rust-lang.rust-analyzer", "tamasfe.even-better-toml"]
```

Use full `<publisher>.<extension>` strings. Repeated IDs, including case-only
variants, are installed once using the first spelling. The task needs an
available VS Code CLI and installs missing extensions; Arch chroot provisioning
defers this to first login. See [VS Code extensions](task-reference.md#vs-code-extensions).

## Private overlays

Keep personal or machine-specific additions in a separate
[Private overlay](private-overlays.md). That guide covers layout, setup,
selection, resource-by-resource merging, scripts, and cleanup. An overlay
extends valid base configuration; it cannot hide conflicting declarations.

## Agent package configuration

Agent packages use YAML fragments under `symlinks/apm/config/`, selected by
`conf/symlinks.toml`. They are separate from `agent-settings.toml` preferences.
Use [AI agent configuration](ai-agent-configuration.md) for package sources, deployment targets,
generated files and cleanup.

## Removing configuration

Deleting a record generally means **stop managing it**, not **undo it**.
Packages, Git/registry/harness values, services and merged system files remain.

For links or overlay scripts, keep the declaration active while running the
intended [remove task](task-reference.md#remove-tasks), then delete it. Symlink removal
materializes matching links as local content rather than restoring a backup.
It also creates a copy for an absent configured target and preserves existing
non-link files/directories.

APM has its own stale-deployment cleanup and
[last-fragment caveat](ai-agent-configuration.md#removing-packages-and-targets).

## Loading and reload behavior

All required files are loaded before tasks are selected. `--only` cannot
bypass a malformed file or incompatible declarations in another section.
When repository synchronization changes content, the CLI restarts with fresh
configuration before later tasks run; this is not a separate selectable reload
task. See the [restart boundary](architecture.md#repository-updates-are-a-restart-boundary).

### Unknown keys are errors

Unknown fields fail loading rather than being ignored: `symlink` instead of
`symlinks`, or `targett` instead of `target`, is an error. Inactive categories
are structurally decoded too, so `[windows]` does not hide a typo on Linux.
String-or-table fields reject a third entry type rather than coercing it.

## Validation

`dotfiles check` covers configuration diagnostics, required files, declared
sources and available analyzers. Its `config-diagnostics` task fails on all
aggregated diagnostics, including warning-severity findings.

The Rust `config_drift` suite checks relationships between real declarations
and tracked sources. Parsing one file or checking one platform does not prove
all category combinations work. See [Choosing coverage](testing.md#choosing-coverage).

## Application preferences

The formats above select and install application sources; they do not describe
every application's own settings syntax. Start with
[Application settings](application-settings.md) for shared shell preferences,
[Arch desktop](arch-desktop.md) for the graphical session, and the
[VS Code](visual-studio-code.md) or [Vim and Neovim](vim-and-neovim.md) guides
for editor behavior and plugins.
