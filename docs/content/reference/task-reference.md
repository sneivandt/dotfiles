---
title: Task reference
description: Find a task by selector, then review its behavior, configuration, prerequisites, and removal limits.
---

# Task reference

A selector identifies a whole task rather than an individual item: `--only symlinks`
selects the Home symlinks task, and your active configuration determines which
links it manages. If you know the change you want but not its selector, start
with [Find the right task](#find-the-right-task).

To inspect the tasks available for your checkout and saved profile:

```sh
dotfiles list
dotfiles list --all
dotfiles list --graph update
```

`dotfiles list` hides tasks ruled out by your platform or config, while `--all`
shows the full public catalog (neither checks runtime prerequisites or changes
your machine). If you haven't set up the launcher or profile yet, see
[Getting started](../guides/getting-started.md).

## Find the right task

| What do you want to do? | Task pages |
|---|---|
| Link application settings or inspect their permissions | [Home symlinks](home-symlinks.md), [File permissions](file-permissions.md) |
| Install applications, AUR helpers, or editor extensions | [System packages](system-packages.md), [Paru package manager](paru-package-manager.md), [AUR packages](aur-packages.md), [VS Code extensions](vs-code-extensions.md) |
| Refresh or prepare custom Arch package recipes | [Custom package versions](custom-package-versions.md), [Custom package repository](custom-package-repository.md) |
| Configure services or administrator-owned files | [Systemd units](systemd-units.md), [System configuration files](system-configuration-files.md) |
| Change Windows preferences or allow file links | [User registry settings](user-registry-settings.md), [Windows Developer Mode](windows-developer-mode.md) |
| Change Git defaults or install contributor hooks | [Global Git settings](global-git-settings.md), [Git hooks](git-hooks.md) |
| Set agent preferences or deploy reusable content | [Agent settings](agent-settings.md), [Agent packages](agent-packages.md) |
| Make dotfiles available in a shell or set the Linux login shell | [Dotfiles launcher](dotfiles-launcher.md), [Shell PATH](shell-path.md), [Shell completions](shell-completions.md), [Login shell](login-shell.md) |
| Sync the dotfiles checkout or run private setup scripts | [Dotfiles repository](dotfiles-repository.md), [Overlay scripts](overlay-scripts.md) |
| Diagnose configuration or source problems | [Validation tasks](#validation-tasks) |

Use the selector catalog below for exact names, or search by task title.

## Catalog overview

These built-in selectors belong to `update`. Availability indicates whether a
task applies to your platform and config, not whether required tools or
privileges are present. Only `git-hooks`, `launcher`, and `symlinks` also
support [removal](#remove-tasks).

| Selector | Task and availability |
|---|---|
| `agent-packages` | [Agent packages](agent-packages.md): active APM fragments |
| `agent-settings` | [Agent settings](agent-settings.md): configured harness settings |
| `aur-packages` | [AUR packages](aur-packages.md): Arch/AUR support and AUR entries |
| `completions` | [Shell completions](shell-completions.md): supported shell registration |
| `custom-package-repository` | [Custom package repository](custom-package-repository.md): Arch/AUR support |
| `custom-package-versions` | [Custom package versions](custom-package-versions.md): Arch/AUR support and dotfiles-managed recipes |
| `developer-mode` | [Windows Developer Mode](windows-developer-mode.md): Windows |
| `file-permissions` | [File permissions](file-permissions.md): supported Unix modes and configured paths |
| `git` | [Global Git settings](global-git-settings.md): configured values or Windows autocrlf cleanup |
| `git-hooks` | [Git hooks](git-hooks.md): repository hook sources |
| `launcher` | [Dotfiles launcher](dotfiles-launcher.md): Linux or Windows |
| `login-shell` | [Login shell](login-shell.md): Linux, outside CI |
| `packages` | [System packages](system-packages.md): configured non-AUR packages |
| `paru` | [Paru package manager](paru-package-manager.md): pacman platform |
| `path` | [Shell PATH](shell-path.md): Linux or Windows |
| `registry` | [User registry settings](user-registry-settings.md): configured Windows values |
| `repository` | [Dotfiles repository](dotfiles-repository.md): eligible checkout |
| `symlinks` | [Home symlinks](home-symlinks.md): configured links |
| `system-files` | [System configuration files](system-configuration-files.md): Linux fragments, outside CI |
| `systemd` | [Systemd units](systemd-units.md): supported platform and configured units, outside CI |
| `vscode-extensions` | [VS Code extensions](vs-code-extensions.md): configured extensions |

## Overlay scripts

Each active private overlay script registers its own `script-<normalized-name>`
task for both `update` and `remove`; see [Overlay scripts](overlay-scripts.md)
for discovery, preview behavior, and the script protocol.

## Remove tasks

`remove` reads **current configuration rather than a historical inventory**, so
keep the original profile, overlay, and source files available until it finishes.

| Selector | What removal does |
|---|---|
| `symlinks` | Replaces managed links with source copies, preserving ordinary files and missing targets (`--restore-missing` also populates absent targets) |
| `git-hooks` | Deletes matching managed hooks while preserving modified hooks and unrelated names |
| `launcher` | Deletes a matching managed launcher while preserving modified copies and leaving `PATH` intact |
| `script-…` | Runs the active script's removal protocol (safety and completeness depend on the script) |

`remove` is not a full backup restore: packages, services, registry/Git/agent
settings, login shell, permissions, completions, WSL config, editor extensions,
and agent deployments are not automatically undone. See
[Home symlinks](home-symlinks.md#removal) for copy rules and links left in
place.

## Validation tasks

`check` runs its own independent validation graph rather than the update graph:

| Selector | What it checks |
|---|---|
| `apm-plugins` | [APM plugin checks](apm-plugin-checks.md): local plugins via `apm pack --dry-run --verbose` |
| `config-diagnostics` | [Configuration diagnostics](configuration-diagnostics.md): aggregated diagnostics (warning-severity findings also fail) |
| `config-files` | [Required configuration files](required-configuration-files.md): required main TOML files (warns if `hooks/` is absent) |
| `psscriptanalyzer` | [PSScriptAnalyzer](psscriptanalyzer.md): PowerShell scripts in the main repository |
| `shellcheck` | [ShellCheck](shellcheck.md): shell scripts in the main repository |
| `symlink-sources` | [Symlink sources](symlink-sources.md): symlink/glob and chmod sources, including inactive categories |

Configuration loads before task filters apply, so `--only` cannot bypass a
malformed [required file](../guides/configuration-basics.md#files). Missing
optional tools surface as unmet skips (turning into failures under
`--fail-on-skip` or `CI`). Local plugin checks don't validate every remote
package or private overlay script. See
[CLI validation](../development/testing.md#cli-validation) for local commands.

## Read a task page

Each task detail page opens with its purpose and a metadata table, then covers
behavior, configuration, preview/apply, and removal. Keep **availability**
distinct from **prerequisites**: a task can apply to your platform yet still
report unmet when a required tool, privilege, or input is missing.

| Detail | How to use it |
|---|---|
| Selector and commands | Pick the task in the right `update`, `remove`, or `check` command |
| Availability or scope | Check which platforms, declarations, and sources it covers |
| Requires success / Runs after | Decide whether predecessor tasks are needed (see [Scheduling model](#scheduling-model)) |
| Implementation | Jump straight to the owning Rust code and tests |
| Configuration | Edit tracked declarations or sources rather than generated or installed copies |
| Preview and removal | Review side effects and retained state before applying or retiring a feature |

Upstream tool links document the underlying tool, not a guarantee that
`dotfiles` exposes every upstream option (for instance, `systemd` supports more
enablement patterns than dotfiles' offline user-unit preparation). Rely on each
task page for dotfiles' supported scope.

## Preview and apply

Commands on task pages assume an installed launcher and saved profile (before
installation, use the checkout wrapper shown in
[Getting started](../guides/getting-started.md)).

Start with `--dry-run` (add `--verbose` to see unchanged items too), resolve
any failures or unmet prerequisites, and then rerun without `--dry-run`. Apply
always re-inspects live state rather than replaying a saved plan.

Selecting a single task doesn't automatically pull in its predecessors; use
`--with-deps` on `update` when needed (see
[Select tasks](../guides/commands-and-usage.md#select-tasks)) and check the
expanded graph first so you don't unexpectedly trigger package installs or a
repo sync.

Remember that `--dry-run` can still access the network, write logs, save
selections, run CLI release checks, and execute overlay preview scripts; see
[Dry-run scope](../guides/commands-and-usage.md#dry-run-scope).

## Scheduling model

Tasks run as soon as their dependencies clear, not in catalog order. Each
update task's summary table lists **Requires success** and **Runs after**
dependencies (which describe task ordering, not required tools or privileges;
`remove` uses its own independent graph):

| Dependency | Meaning when both tasks are selected |
|---|---|
| `requires <-` (`blocking` in JSON) | Predecessor failure or unmet work blocks the dependent |
| `after <-` (`after` in JSON) | Wait, then inspect this task even if the predecessor failed |

For example, `aur-packages` requires `paru` to succeed, whereas `systemd`
merely waits after `packages` and `symlinks` so an unrelated package failure
doesn't stop it from managing already-installed units.

Package waits for `systemd`, `vscode-extensions`, and `agent-packages` are also
conditional: a readiness probe can release the wait early if the required tool
is already available. Inspect your live graph with:

```sh
dotfiles list --graph update
dotfiles list --graph remove
dotfiles list --graph check
```

Dependency edges only govern tasks that are selected together; pass
`--with-deps` on `update` (or `list --graph update`) to expand predecessors
automatically (`remove` and `check` don't accept `--with-deps`). See
[Task discovery output](cli-reference.md#task-discovery-output) for graph
notation.

Listings and completions display canonical selectors, while `--only` and
`--skip` still accept legacy aliases (`apm` for `agent-packages`, `paru-config`
for `custom-package-repository`, `shell` for `login-shell`, `config-warnings`
for `config-diagnostics`, `local-package-versions` for
`custom-package-versions`, `local-package-repository` for
`custom-package-repository`, and historical full console labels).

Independent ready tasks run in parallel by default (logging rows as they
finish; pass `--no-parallel` for sequential runs). Tasks can finish unchanged,
not applicable, or unmet rather than changed, and a failure never rolls back
completed work. See [Architecture](../development/architecture.md#requirements)
for how readiness probes and conditional scheduling work under the hood.
