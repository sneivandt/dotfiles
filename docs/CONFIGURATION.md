# Configuration reference

Edit desired state in [`conf/`](../conf/), application files in
[`symlinks/`](../symlinks/), and administrator-file fragments in
[`system/`](../system/). Do not edit generated output to make a persistent
configuration change. Installed application files may be live links into this
checkout, so an edit can take effect before another `dotfiles install`.

This guide describes file formats and their consequences. See
[Usage](USAGE.md) for commands, [Profiles](PROFILES.md) for selection, and
[APM](APM.md) for AI package manifests.

## Making a configuration change

1. Find the owning file in the table below. For an application setting, edit
   its existing source rather than adding a CLI task.
2. Choose the narrowest category section that should receive the change.
   Keep a source file and the package or service that uses it in compatible
   categories.
3. Check the path rules for that file. `symlinks`, `chmod`, system fragments,
   and overlay scripts do **not** share one path convention.
4. Validate the configuration, including the overlay if used, and preview the
   affected task against the current checkout. Follow
   [Testing](TESTING.md#cli-validation) and
   [dry-run guidance](TESTING.md#dry-run-testing).
5. Apply only after reviewing the plan. Changing or removing a declaration is
   not a general rollback mechanism; see [removing configuration](#removing-configuration).

Examples below show file syntax, not additional declarations to paste alongside
an existing section with the same name.

## Files

All nine main TOML files are required, even on platforms where their tasks do
not apply. An empty file is valid when nothing is configured.

| File | Section field | What it manages |
|---|---|---|
| [`symlinks.toml`](../conf/symlinks.toml) | `symlinks` | Links from repository sources into the home directory |
| [`packages.toml`](../conf/packages.toml) | `packages` | Arch packages, AUR packages, and Windows winget IDs |
| [`git-config.toml`](../conf/git-config.toml) | `settings` | Global Git key/value settings |
| [`agent-settings.toml`](../conf/agent-settings.toml) | `settings` | Selected Copilot JSON and Codex TOML keys |
| [`chmod.toml`](../conf/chmod.toml) | `permissions` | Unix file and directory modes |
| [`registry.toml`](../conf/registry.toml) | Named records with `path` and `values` | Windows current-user registry values |
| [`systemd-units.toml`](../conf/systemd-units.toml) | `units` | User and system unit enablement and runtime state |
| [`system-files.toml`](../conf/system-files.toml) | `files` | Structured merges into files below `/etc` |
| [`vscode-extensions.toml`](../conf/vscode-extensions.toml) | `extensions` | VS Code extension IDs |

An overlay may additionally contain `conf/scripts.toml`. That file is not loaded
from the main repository.

## Category sections

Except for registry records, TOML files group entries under category names:

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

The accepted tags are `base`, `desktop`, `linux`, `windows`, `arch`, and `wsl`.
A hyphen means **AND**: `[arch-desktop]` requires both categories, not either.
Custom or misspelled tags are errors.

### Profiles

`base` is always active. The `desktop` profile adds the `desktop` category;
the CLI supplies platform categories. Use the profile for the machine's role,
not its operating system. Selection precedence and persistence are documented
in [Profiles](PROFILES.md).

## Path conventions

| Declaration | Resolves relative to | Example result |
|---|---|---|
| Symlink `source` | Declaring repository's `symlinks/` | `config/git/config` reads `symlinks/config/git/config` |
| Symlink without `target` | Home, with a leading dot added | `config/git/config` becomes `~/.config/git/config` |
| Explicit symlink `target` | Home, exactly as written | `Documents/file` becomes `~/Documents/file` |
| Permission `path` | Home, with a leading dot added | `ssh/config` becomes `~/.ssh/config` |
| System-file `source` | Declaring repository's `system/` | `pacman.conf` reads `system/pacman.conf` |
| Omitted system-file `target` | `/etc/` plus `source` | `pacman.conf` becomes `/etc/pacman.conf` |
| Overlay script `path` | Overlay root | `scripts/setup.sh` reads `<overlay>/scripts/setup.sh` |

Use `/` in portable configuration paths. An overlay entry retains its source
repository; it does not read a same-named source from the main checkout.

## Symlinks

A bare string uses the dot-prefixed target convention:

```toml
[base]
symlinks = ["config/git/config", "ssh/config"]
```

Use a table for a different target, including Windows locations that must not
have a leading dot:

```toml
[windows]
symlinks = [
  { source = "config/nuget/nuget.config", target = "AppData/Roaming/NuGet/nuget.config" },
]
```

The same source can serve multiple distinct targets. For example, the Vim tree
is linked directly to both `~/.vim` and `~/.config/nvim`; an intermediate
forwarding file or repository symlink is unnecessary.

Sources must remain within their owning `symlinks/` tree. Targets must name a
descendant of home, not home itself. Current-directory components are normalized:
`./bashrc` still targets `~/.bashrc`, while `.` is invalid. Duplicate targets and
parent/child target overlaps are rejected. Comparison ignores redundant
separators and `.` components, and is case-insensitive on Windows.
Targets that alias the source entry through existing directory links are
rejected rather than replacing the source itself.

**Back up existing files before applying.** Installation warns and replaces a
regular file or empty directory at a managed target without making a backup.
A nonempty real directory fails rather than being recursively deleted. Read
the [installation and uninstall behavior](USAGE.md) before managing an existing
application configuration.

### Glob patterns

Use a complete `*` path segment to manage each entry separately:

```toml
[base]
symlinks = ["apm/plugins/*"]
```

The rules are deliberately narrower than shell globs:

- `*` matches one complete segment, including dot-prefixed files and directories.
  Partial patterns such as `plugins/*.yml` and recursive `**` are errors.
- Expansion does not descend through symlinked directories.
- A pattern must match at least one entry; an empty match is an error.
- Matched names must be valid UTF-8; invalid names fail rather than being
  silently replaced with a different configuration path.
- Matches are sorted, then checked for the same target conflicts as explicit
  entries. Expansion happens after category filtering.

If an explicit target contains wildcards, source and target must have the same
number. Captures are substituted in order:

```toml
[base]
symlinks = [
  { source = "skills/*", target = ".copilot/skills/*" },
]
```

That example requires a `symlinks/skills/` directory with at least one entry.
For this repository's reusable agent content, prefer the existing
[APM deployment](APM.md) rather than a second deployment path.

## Packages

```toml
[arch]
packages = [
  "git",
  "ripgrep",
  { name = "apm-bin", aur = true },
]

[windows]
packages = ["Git.Git", "Microsoft.PowerShell"]
```

Regular Arch packages use pacman; `aur = true` sends a package to the separate
AUR task. Windows entries are winget identifiers. A package declaration is
platform-specific desired state, not a portable package-name translation.
When configured packages are missing, the pacman task uses `-Syu`: an ordinary
install can synchronize package databases and upgrade installed system packages
as well as add missing ones. See the [task reference](TASKS.md) for provider
prerequisites and update behavior.

## Git settings

```toml
[windows]
settings = [
  { key = "core.longpaths", value = "true" },
]
```

Values are strings, including booleans written as `"true"` or `"false"`.
These entries converge global Git settings. Shared Git configuration files and
platform includes instead live under `symlinks/config/git/`; do not introduce
contradictory values in the two places.

## Agent harness settings

```toml
[base]
settings = [
  { target = "copilot", key = "footer.showBranch", value = true },
  { target = "codex", key = "model_reasoning_effort", value = "medium" },
]
```

| Target | Managed document |
|---|---|
| `copilot` | `~/.copilot/settings.json` |
| `codex` | `~/.codex/config.toml` |

Keys are dot-separated paths; values use TOML types. Manage the narrowest key
needed so unrelated user and harness-owned settings remain untouched. Existing
documents must be JSON objects or TOML tables; malformed documents are reported,
not replaced with an empty one.
Conflicting values for the same harness and exact key are rejected before any
settings document is changed, including conflicts across main and overlay.

These are harness preferences, not plugin declarations. Use [APM](APM.md) for
skills, instructions, hooks, and MCP configuration. Removing a setting stops
managing it but does not delete its stored value.

## File permissions

```toml
[linux]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/zsh" },
]
```

Modes are three- or four-digit octal strings. Paths use the **dot-prefixed**
home convention: `ssh/config` means `~/.ssh/config`, not `~/ssh/config`.
Current-directory components are normalized: `./ssh/config` has the same target.
Absolute paths, parent traversal, and paths naming home itself are invalid.
Missing targets are not created by this task.

Directory entries apply recursively. Directories retain traversal access;
ordinary files have execute bits cleared. Give an executable file its own
entry rather than expecting a directory's `755` to make every file executable.
Explicit descendant entries override recursive ancestors regardless of entry
order or parallel execution; ancestor resources leave those targets untouched.
Permissions applied through managed links can affect their repository sources.

## Registry

Registry section names are labels, **not categories**. All records apply on
Windows regardless of profile:

```toml
[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

Only `HKCU:\` is supported. TOML integers and booleans become `REG_DWORD`;
parseable hexadecimal strings such as `"0x0E"` also become DWORDs. Ordinary
strings become `REG_SZ`, so `"14"` is a string while `14` is a DWORD.
The file is still parsed on non-Windows platforms to catch structural errors,
but its entries are not applied there.

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

A string means `scope = "user"` and `enabled = true`. A table must specify
`name` and `scope` (`user` or `system`); `enabled` defaults to `true`.
Supported unit suffixes are `.service`, `.timer`, `.socket`, `.target`, `.path`,
`.mount`, `.automount`, and `.swap`.
User unit files are usually provided by symlinks before this task runs.
System-scope changes use `sudo`.

With a live manager, enabled units are enabled and started; disabled units are
disabled and stopped. A static unit may have no enablement links, but
`enabled = false` still requires it to be stopped. This does not mask the unit
or stop another unit from starting it later.

Without a user service manager, user-unit enablement is prepared for the next
login, not started immediately. The search order is `~/.config/systemd/user`,
`/etc/systemd/user`, `/run/systemd/user`, `/usr/local/lib/systemd/user`,
`/usr/lib/systemd/user`, then `/lib/systemd/user`. Enablement links are created
in the user's unit directory, including for packaged units. Links to managed
units retain their installed home path so materializing them during uninstall
does not leave an enablement link pointing into a removed checkout.
Offline disabling removes matching existing `.wants` and `.requires` links,
even if the unit file has disappeared or its `[Install]` directives changed.
Offline enablement honors empty `WantedBy=` and `RequiredBy=` assignments,
which reset only their respective lists of earlier relationships.

## System files

Use `system-files.toml` for structured changes to administrator-owned files,
not a home-directory symlink:

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

Omitting `target` selects `/etc/<source>`. Set it explicitly only when the
destination differs. Sources belong under the declaring repository's `system/`;
targets must be absolute paths strictly below `/etc`. Neither may contain `..`.
Duplicate active targets are errors, including across main and overlay.

| `merge` | Managed part of the destination |
|---|---|
| `toml` | Fragment keys, merging tables recursively |
| `ini` | Assigned keys and bare flags within sections, including Pacman options |
| `pam` | Matching module rules, inserting fragment rules after the final rule in each facility stack |

Unrelated settings are preserved. These are privileged changes; review the
fragment and preview before applying. Removing the declaration does not restore
the old file.
PAM merges require an existing facility stack; a stack consisting solely of
the managed module is valid and remains convergent on subsequent runs.
Ownership matches the facility and actual module field, including after
bracketed controls; module names used as arguments or included stack names
do not make an unrelated rule managed.

## VS Code extensions

```toml
[desktop]
extensions = ["rust-lang.rust-analyzer", "tamasfe.even-better-toml"]
```

Use full `<publisher>.<extension>` identifiers. The task uses an available
VS Code CLI to install missing extensions.

## Overlays

An overlay is a second repository for private or machine-specific desired
state. It uses the same `conf/`, `symlinks/`, and `system/` layout, plus optional
scripts. Do not copy private content, credentials, or machine logs into this
public repository.

### Selection and persistence

Precedence is `--overlay <PATH>`, then `DOTFILES_OVERLAY`, then repository-local
Git configuration `dotfiles.overlay`. Relative selections resolve against the
invoking process's working directory, **not** the dotfiles root.

An explicit selection is persisted as an absolute path for future runs.
Previously saved relative paths remain relative to the invoking directory until
replaced. Read-only task discovery resolves a selection without saving it.
The startup header shows the resolved overlay path.

An explicit linked Git worktree prompts for confirmation before use or
persistence. The prompt defaults to no; a non-interactive invocation rejects
the new worktree selection, including `--non-interactive` or CI policy on an
attached terminal. An ordinary checkout with a `.git` directory does not need
that confirmation.

### Merge rules

Ordinary TOML entries are appended main-first, overlay-second. Missing overlay
files contribute nothing. This is **not a general override mechanism**: source
ownership is retained, conflicts are still checked, and two declarations of
one target do not automatically become one declaration.

Keep one clear desired value per target. APM YAML fragments have their own
[merge rules](APM.md#configuration-fragments), not TOML append semantics.

### Conflicting desired state

Active Git settings and Windows registry entries cannot declare different
values for the same target. Loading fails before tasks run, even with `--only`
or `--dry-run`. Diagnostics identify both declarations using
`git.conflicting-values` or `registry.conflicting-values`.

Identical declarations are allowed. Git section and variable names are
case-insensitive; subsection names are case-sensitive and values are compared
literally. Registry paths and value names are case-insensitive; both value type
and data must agree. DWORD `14` and `"0x0E"` agree; DWORD `14` and string `"14"`
do not. Inactive Git categories and registry entries off Windows do not
participate in these conflict checks.

## Overlay scripts

Use a script only for private work that the declarative configuration does not
already express. Define it in the overlay's `conf/scripts.toml`:

```toml
[[base.scripts]]
name = "Configure private workstation"
path = "scripts/configure-workstation.ps1"
description = "Converge private workstation settings"
```

Names must be nonempty and normalize to distinct task selectors. This example
becomes `script-configure-private-workstation`. Paths must stay inside the
overlay, including after resolving symlinks. Scripts run with the overlay root
as their working directory. `.ps1`, matched case-insensitively, uses `pwsh` (or
Windows PowerShell when available on Windows); other extensions use `sh`.

Implement the complete protocol:

| Invocation | Required behavior |
|---|---|
| `--check` | Inspect only. Exit `0` if configured, `1` if absent/needs applying; any other failure stops the task. |
| No flag | Apply idempotently; return nonzero on failure. |
| `--dryrun` | Describe the intended application without changing state. Note the spelling: no hyphen between `dry` and `run`. |
| `--remove` | Remove the script's managed state conservatively; return nonzero on failure. |

Install checks first and applies or previews only if needed. Uninstall uses the
same check: `0` means there is state to remove, `1` means nothing to remove.
An uninstall dry run checks and reports the removal but never invokes
`--remove` or `--dryrun`.

**Script dry-run safety is cooperative, not sandboxed.** Both inspection and
preview must be read-only. Avoid secrets in output: script output can appear
in the console and run log. Active scripts are discovered at the configuration
reload boundary and appear in [task discovery](TASKS.md).

## APM configuration

APM packages use YAML fragments under `symlinks/apm/config/`, selected by
`conf/symlinks.toml`. They are distinct from harness preferences in
`agent-settings.toml`. See [APM](APM.md) for source ownership, target selection,
updates, and generated files.

## Removing configuration

Removing a record usually means "stop managing this," not "undo it." Packages,
Git settings, registry values, service state, agent settings, and merged system
files are not generally removed just because their declarations disappear.

For a managed link or overlay script, keep its declaration active while
performing the intended [uninstall](USAGE.md), then remove the declaration.
Uninstall materializes matching managed links as local copies; it does not
restore arbitrary pre-install state. It can also create a local copy when the
configured target is absent; existing non-link files and directories are
preserved. APM has separate native stale-deployment cleanup and a
[last-fragment caveat](APM.md#removing-packages-and-targets).

## Loading and reload behavior

The loader resolves the profile, decodes main and optional overlay files,
selects active categories, expands symlink globs, and rejects structural and
target conflicts before tasks are built. Aggregate diagnostics cover additional
domain and cross-file rules. `--only` is task selection, not a way to skip
configuration loading.

If repository synchronization changes tracked content, **Reload configuration**
loads and validates the new state before later tasks use it. See
[Architecture](ARCHITECTURE.md) for that runtime boundary.

### Unknown keys are errors

Unknown section fields and entry fields fail loading rather than being ignored.
For example, `symlink` instead of `symlinks`, or `targett` instead of `target`,
is an error. Structural parsing includes inactive categories and platforms;
putting a typo in `[windows]` does not hide it on Linux.

String-or-table entries are strict too: a value of some third type is not
silently coerced. Diagnostics include source context so the declaration can
be corrected rather than worked around.

## Validation

`dotfiles check` covers loader diagnostics, required files, declared sources,
and available APM/script analyzers. The `config_drift` Rust suite checks
relationships among the real configuration and tracked sources. Neither a
successful parse nor a single-platform run proves every category works.
Use [Testing](TESTING.md#choosing-coverage) to select coverage.

## Desktop application configuration

Desktop appearance and keybindings are application source files, not additional
TOML schemas.

### Arch desktop appearance

Quickshell's shared colors, typography, spacing, and motion live in
[`Theme.js`](../symlinks/config/quickshell/Theme.js). `ShellPopup.qml` owns popup
placement, dismissal, scrolling, and focus; one bar popup is open at a time
across monitors. The bar reserves space for tiled windows, sits below floating
windows, and hides on fullscreen workspaces.

The volume menu selects the default output and permits amplification to 150%,
marking 100%. The network menu uses NetworkManager for saved networks and
password entry; enterprise setup stays in the connection editor. Logging out,
restarting, and shutting down require confirmation; locking is immediate.
Failed market refreshes retain previous quotes and show stale/error status.

Quickshell watches linked sources and keeps the last valid shell on reload
failure. Editing this checkout can therefore reload the live UI. For deliberate
manual reloads of other components, Hyprland uses `hyprctl reload config-only`
and `hyprctl configerrors`; mako uses `makoctl reload`. Fuzzel reads its config
on launch, and existing GTK applications may need reopening.
Retain the simple Hyprlock background and disabled animations while the
[documented DPMS workaround](../symlinks/config/hypr/hyprlock.conf) is needed.

### Hyprland media keys

[`binds.lua`](../symlinks/config/hypr/conf/binds.lua) uses standard `XF86` symbols,
not physical F-key positions. Keyboard Fn mode determines which symbols are
sent. The fallback shortcuts are:

| Shortcut | Action |
|---|---|
| Super + F1 | Toggle output mute |
| Super + F2 / F3 | Lower / raise volume by 5% |
| Super + F4 | Toggle microphone mute |
| Super + F5 / F6 | Lower / raise display brightness by 5% |

Volume follows the default output through `pactl` and can exceed 100%.
Playback uses `playerctl`. Dedicated media keys work while locked; fallback
shortcuts do not.

[`brightness.sh`](../symlinks/config/hypr/scripts/brightness.sh) uses
`brightnessctl` for display backlights, never keyboard LEDs, and keeps brightness
above zero. No backlight means no action. External monitors generally need
separate DDC/CI configuration unless their driver exposes a backlight.
