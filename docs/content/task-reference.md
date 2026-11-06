---
title: Task reference
description: Find a task by selector, then review its behavior, configuration, prerequisites, and removal limits.
---

# Task reference

Find the task below, then open its page for configuration examples and the
changes it makes. A selector chooses a kind of work, such as `symlinks` or
`packages`, rather than one file or package inside that task.

A task brings one domain of desired state into place, such as home links or
service enablement. Its declarations choose the individual items. The task
page explains the underlying tool, what dotfiles manages on top of it, and
what it deliberately leaves alone. `check` tasks inspect configuration or
source instead of applying that desired state.

Start with the installed launcher and a selected profile:

```sh
dotfiles list
dotfiles list --all
dotfiles list --graph update
```

The default list hides tasks that cannot apply to your platform or
configuration. `--all` shows the full public catalog. Listing does not check
runtime prerequisites or apply changes. See [Getting started](getting-started.md)
if the launcher or profile is not configured yet.

## Catalog overview

These are the built-in update selectors. Follow a label for details. The table
shows which configuration or platform makes each task relevant; required tools
or privileges may still be missing. Only `git-hooks`, `launcher`
and `symlinks` also belong to [remove](#remove-tasks).

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

Each active private script becomes a separate `script-<normalized-name>` task
for update and remove. [Overlay scripts](overlay-scripts.md) explains discovery,
preview behavior, and the configuration protocol.

## Remove tasks

Remove uses **current configuration, not a historical inventory**. Keep the
original profile, overlay and sources available until it completes.

| Selector | What removal does |
|---|---|
| `symlinks` | Replace managed links with source copies. Preserve ordinary files and missing targets; `--restore-missing` also fills absent targets. |
| `git-hooks` | Delete matching managed hooks; preserve modified hooks and unrelated names. |
| `launcher` | Delete a matching managed launcher; preserve modified launchers and leave PATH unchanged. |
| `script-…` | Run the active script's removal protocol. Its safety and completeness depend on the script. |

This is not restoration from backups. Packages, services, registry/Git/harness
values, shell selection, permissions, completions, WSL settings, editor
extensions and agent deployments are not reversed by static remove.

For file-copy details and links that removal must leave in place, see
[Home symlinks](home-symlinks.md#removal).

## Validation tasks

`check` has its own task set; it does not execute the update graph.

| Selector | What it checks |
|---|---|
| `config-diagnostics` | [Configuration diagnostics](configuration-diagnostics.md): Aggregated diagnostics; warning-severity findings also fail |
| `symlink-sources` | [Symlink sources](symlink-sources.md): Symlink/glob and chmod sources, including inactive categories |
| `config-files` | [Configuration files](configuration-files.md): Required main TOML files; warns if `hooks/` is absent |
| `apm-plugins` | [APM plugin checks](apm-plugin-checks.md): Local plugins through `apm pack --dry-run --verbose` |
| `shellcheck` | [ShellCheck](shellcheck.md): Shell scripts in the main repository |
| `psscriptanalyzer` | [PSScriptAnalyzer](psscriptanalyzer.md): PowerShell scripts in the main repository |

The check command loads configuration before filtering. `--only` cannot bypass a
malformed [required file](configuration-basics.md#files).

Missing optional tools cause visible unmet skips; `--fail-on-skip` or CI makes
these unsuccessful. Local plugin checks do not validate every remote package
or private overlay script.

Use [CLI validation](testing.md#cli-validation) for commands, then select
additional integration/configuration-drift coverage as appropriate.

## Read a task page

Each task page starts with its purpose and a selection summary, followed by
its behavior and relevant declaration format. Read availability separately
from prerequisites: a task can be selected for your platform but remain unmet
because a required tool, privilege, or input is absent.

| Detail | How to use it |
|---|---|
| Selector and commands | Choose the task in the right update, remove, or check command |
| Availability or scope | Understand which platforms, declarations, and sources it covers |
| Requires success / Runs after | Decide whether to include predecessor tasks; see the scheduling model below |
| Implementation | Find the owning code and its tests when changing behavior |
| Configuration | Edit the declaration or source, not a generated output or installed copy |
| Preview and removal | Review side effects and retained state before applying or retiring the feature |

Tool references describe the underlying tool, not a promise that dotfiles
supports every upstream option. For example, systemd supports more enablement
relationships than dotfiles' offline user-unit preparation does. Prefer the
task's documented scope when planning an update, and the linked upstream
documentation when learning the tool itself.

## Scheduling model

Tasks run when their dependencies allow, not in catalog order. Each update task's
summary table distinguishes **Requires success** from **Runs after**, using the
terms below. These rows describe task dependencies, not required tools or privileges;
read the task's behavior and availability too. Removal uses its own graph and does
not inherit update dependencies.

| Dependency | Meaning when both tasks are selected |
|---|---|
| `requires <-` (`blocking` in JSON) | Predecessor failure or unmet work blocks the dependent |
| `after <-` (`after` in JSON) | Wait, then inspect this task even if the predecessor failed |

For example, AUR installation requires a working Paru bootstrap. Systemd only
waits for package and symlink work: an unrelated failure there must not prevent
it inspecting units that are available.

Package waits for systemd, VS Code extensions, and agent packages are conditional:
a readiness probe can release the wait before package installation finishes. The
task still checks its own prerequisites. To see the selected graph for your current
platform, profile, and overlay:

```sh
dotfiles list --graph update
dotfiles list --graph remove
dotfiles list --graph check
```

A dependency row describes ordering when both tasks are selected. Selecting a task
alone does not automatically select its predecessors; `--with-deps` expands the
selection for update and remove.

Use [Task selection](commands-and-usage.md#select-tasks) for filtering and
dependency expansion. The [discovery reference](cli-reference.md#task-discovery-output)
defines graph markers and plain/JSON output. A graph describes selection,
not the changes a run will make.

Listings and completions use canonical selectors. For compatibility, `apm`,
`paru-config`, `shell` and `config-warnings` still select `agent-packages`,
`custom-package-repository`, `login-shell` and `config-diagnostics`, respectively.
Previous full console labels remain accepted by `--only` and `--skip` too.
The former `local-package-versions` and `local-package-repository` selectors
also remain aliases for `custom-package-versions` and `custom-package-repository`.

Independent ready tasks may run concurrently, with console rows in completion
order. `--no-parallel` disables parallel execution. Tasks may finish current,
not applicable or unmet rather than changed; failure is not a rollback.
See [Architecture](architecture.md#requirements) for runtime readiness
assessment and conditional scheduling.
