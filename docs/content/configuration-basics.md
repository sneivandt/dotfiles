---
title: Configuration basics
description: Find configuration files and understand categories, paths, loading, and changes that remain after a declaration is removed.
---

# Configuration basics

These rules apply across configuration files. Individual
[task pages](task-reference.md) describe what runs and how to configure it.

Declarations under `conf/` choose what dotfiles manages. Each [task
page](task-reference.md) combines its behavior with the format of its declarations. This
page covers the rules shared by those files.

To change an application's own settings, edit its tracked source under
[symlinks/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/), not the
installed copy. Live links can make source edits take effect immediately. [Application
settings](application-settings.md) helps you find the right source. Administrator-file
fragments live under [system/](https://github.com/sneivandt/dotfiles/tree/main/system/).

## Files

Every main file listed here is required, even on platforms where its task does not
apply. A required file may be empty.

| File | Purpose |
|---|---|
| [symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml) | [Home symlinks](home-symlinks.md) |
| [packages.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/packages.toml) | [System packages](system-packages.md) and [AUR packages](aur-packages.md) |
| [git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml) | [Global Git settings](global-git-settings.md) |
| [agent-settings.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/agent-settings.toml) | [Agent settings](agent-settings.md) |
| [chmod.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/chmod.toml) | [File permissions](file-permissions.md) |
| [registry.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/registry.toml) | [User registry settings](user-registry-settings.md) |
| [systemd-units.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/systemd-units.toml) | [Systemd units](systemd-units.md) |
| [system-files.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) | [System configuration files](system-configuration-files.md) |
| [vscode-extensions.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml) | [VS Code extensions](vs-code-extensions.md) |

Private overlays may also provide
[conf/scripts.toml](private-overlays.md#overlay-scripts). The main repository's
`conf/scripts.toml` is not loaded.

## Making a configuration change

1. Find the owning file in the table above, then open its task page for the declaration format.
2. Choose the category that should receive the change. A package, its linked
   configuration and its service should have compatible categories.
3. Check the [path conventions](#path-conventions). Some entries add a leading
   dot to the home destination; explicit symlink targets do not.
4. Validate the selected configuration and preview the relevant
   [task](task-reference.md), as in the example below.
5. Back up existing targets before applying. Removing a declaration usually
   stops management; it does not undo an earlier change.

Examples are valid TOML excerpts, not replacements for the complete configuration.
Extend the existing section rather than adding a second table with the same name.

For a symlink declaration, use this sequence with the installed launcher for your
intended checkout and a saved profile:

```sh
dotfiles check --only config-diagnostics,symlink-sources,config-files
dotfiles update --only symlinks --dry-run
```

Resolve failed or skipped checks before applying. The preview should show the intended
destinations under Home symlinks; add `--verbose` to see links that are already current.
For another kind of change, select its task instead. The preview uses current checkout
content, but still has the [documented side
effects](commands-and-usage.md#dry-run-scope).

Not installed yet? [Getting started](getting-started.md) covers the wrapper, profile
selection, backups, and the first apply. Contributors changing the loader or
configuration model should also use [Testing](testing.md#cli-validation).

## Category sections

Except for registry records, configuration is grouped by category:

```toml title="conf/symlinks.toml (excerpt)"
[base]
symlinks = ["config/git/config"]

[windows]
symlinks = ["wslconfig"]

[arch-desktop]
symlinks = ["config/hypr/hyprland.lua"]
```

The available tags are `base`, `desktop`, `linux`, `windows`, `arch` and `wsl`. A hyphen
means **AND**: `[arch-desktop]` needs both categories. Unknown or misspelled tags are
errors, not new custom categories.

`base` is always active. Selecting the desktop profile adds `desktop`; the CLI supplies
platform categories. Profiles describe a machine's role, not its operating system. See
[Profiles](profiles-and-categories.md) for selection precedence, persistence and
platform combinations.

## Path conventions

Symlink sources are relative to the declaring repository's `symlinks/` directory;
system-file sources are relative to its `system/` directory. An overlay entry uses the
overlay's directories, not the main checkout's. Script paths are relative to the overlay
root.

- A symlink source `"config/git/config"` becomes `~/.config/git/config`,
  with a leading dot added.
- An explicit symlink `target = "Documents/file"` becomes `~/Documents/file`,
  without adding a dot.
- A permission `path = "ssh/config"` refers to `~/.ssh/config`.
- A system-file `source = "pacman.conf"` defaults to `/etc/pacman.conf`;
  `target` can override it.

Use `/` in portable paths. Home-relative paths are not shell expressions: write the
relative path, not a `~/` prefix.

## Private overlays

Keep personal or machine-specific additions in a separate [Private
overlay](private-overlays.md). That guide covers layout, setup, selection,
resource-by-resource merging, scripts, and cleanup. An overlay extends valid base
configuration; it cannot hide conflicting declarations.

## Removing configuration

Deleting a record generally means **stop managing it**, not **undo it**. Packages,
Git/registry/harness values, services and merged system files remain.

For links or overlay scripts, keep the declaration active while running the intended
[remove task](task-reference.md#remove-tasks), then delete it. Symlink removal replaces
matching managed links with local copies of their current source contents rather than
restoring a backup. Existing ordinary files and directories are preserved. Missing
targets stay missing unless removal uses `--restore-missing`.

APM has its own stale-deployment cleanup and [last-fragment
caveat](agent-configuration.md#removing-packages-and-targets).

## Loading and reload behavior

Normal update, remove, and check runs load all required files before selecting
tasks. `--only` cannot bypass a malformed file or incompatible declarations in
another section during those runs.

There is one recovery exception: `dotfiles update --only repository`, without
other selectors or skips, can synchronize the checkout without loading its TOML
configuration. Use it when an upstream configuration fix is available, as
described in [Dotfiles repository](dotfiles-repository.md#what-it-does).
It does not bypass Git's readiness and safety checks.

When repository synchronization changes content in a normal update, the CLI
restarts with fresh configuration before later tasks run; this is not a separate
selectable reload task. See the [restart
boundary](architecture.md#repository-updates-are-a-restart-boundary).

### Unknown keys are errors

Unknown fields fail loading rather than being ignored: `symlink` instead of `symlinks`,
or `targett` instead of `target`, is an error. Fields and value types are checked in
inactive categories too, so `[windows]` does not hide a typo on Linux. Fields accepting
strings or tables reject other entry types.

## Validation

`dotfiles check` covers configuration diagnostics, required files, declared sources and
available analyzers. Its `config-diagnostics` task fails on all aggregated diagnostics,
including warning-severity findings.

The Rust `config_drift` suite checks relationships between real declarations and tracked
sources. Parsing one file or checking one platform does not prove all category
combinations work. See [Choosing coverage](testing.md#choosing-coverage).

## Application preferences

The formats above select and install application sources; they do not describe every
application's own settings syntax. Start with [Application
settings](application-settings.md) to find common settings sources, [Arch
desktop](arch-desktop.md) for the graphical session, and the [VS
Code](application-settings.md#visual-studio-code) or [Vim and
Neovim](application-settings.md#vim-and-neovim) sections for editor behavior and
plugins.
