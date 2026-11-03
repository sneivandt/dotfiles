---
title: Task reference
description: Find task selectors, dependencies, managed state, and the boundaries of update, remove, and validation tasks.
---

# Task reference

A task selector chooses a kind of work, such as `symlinks` or `packages`.
It does not choose one file, package or service inside that task. Entries come
from the selected [configuration](configuration-reference.md), profile and overlay.

Start with read-only discovery using your configured checkout and profile.
Pass `--profile base` or `--profile desktop` if you have not selected one yet:

```bash
dotfiles list
dotfiles list --all
dotfiles list --graph update
```

The default list hides tasks that cannot apply to the selected platform or
configuration. `--all` shows the complete public catalog. Neither listing
checks runtime prerequisites or applies changes, so a listed task may still
find its tool unavailable when executed.

For command syntax and preview side effects, see [Usage](commands-and-usage.md#select-tasks).

## Catalog overview

These are the built-in update selectors. Follow a label for details. The table
shows which configuration or platform makes each task relevant; required tools
or privileges may still be missing. Only `git-hooks`, `launcher`
and `symlinks` also belong to [remove](#remove-tasks).

| Selector | Console label and availability |
|---|---|
| `agent-packages` | [Agent packages](#agent-packages): active APM fragments |
| `agent-settings` | [Agent settings](#agent-settings): configured harness settings |
| `aur-packages` | [AUR packages](#aur-packages): Arch/AUR support and AUR entries |
| `completions` | [Shell completions](#shell-completions): supported shell registration |
| `custom-package-repository` | [Custom package repository](#custom-arch-packages): Arch/AUR support |
| `custom-package-versions` | [Custom package versions](#custom-arch-packages): Arch/AUR support and dotfiles-managed recipes |
| `developer-mode` | [Windows Developer Mode](#windows-developer-mode): Windows |
| `file-permissions` | [File permissions](#file-permissions): supported Unix modes and configured paths |
| `git` | [Global Git settings](#global-git-settings): configured values or Windows autocrlf cleanup |
| `git-hooks` | [Git hooks](#git-hooks): repository hook sources |
| `launcher` | [Dotfiles launcher](#dotfiles-launcher): Linux or Windows |
| `login-shell` | [Login shell](#login-shell): Linux, outside CI |
| `packages` | [System packages](#system-packages): configured non-AUR packages |
| `paru` | [Paru package manager](#paru-package-manager): pacman platform |
| `path` | [Shell PATH](#shell-path): Linux or Windows |
| `registry` | [User registry settings](#user-registry-settings): configured Windows values |
| `repository` | [Dotfiles repository](#dotfiles-repository): eligible checkout |
| `symlinks` | [Home symlinks](#home-symlinks): configured links |
| `system-files` | [System configuration files](#system-configuration-files): Linux fragments, outside CI |
| `systemd` | [Systemd units](#systemd-units): supported platform and configured units, outside CI |
| `vscode-extensions` | [VS Code extensions](#vs-code-extensions): configured extensions |

## Update tasks

### Windows Developer Mode

Enables the machine-level Developer Mode policy when unset, requiring
Administrator rights. Home symlinks waits with an ordering-only edge, so
unavailable Developer Mode does not block links that need no change or
directory junctions that can be created without it.

Pending file symlinks may still need their own elevation. The CLI normally
stays unelevated and delegates only affected tasks.
See [Windows elevation](windows-and-wsl.md#elevation).

### Dotfiles launcher

Writes a small launcher script that calls this checkout's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

It repairs stale content; it does not copy the Rust binary. Moving the checkout
can invalidate the recorded path.

### Shell PATH

Requires the launcher. Adds `~/.local/bin` through a managed export in
`~/.profile` on Linux, or the user PATH on Windows. Open a new shell to use
the persisted change.

Existing PATH entries are preserved. Remove leaves the PATH addition in place.

### Dotfiles repository

Synchronizes the main checkout and an overlay with a `.git` entry. Tracked
local changes prevent synchronization; untracked files are ignored by that
readiness check. Detached HEAD is inapplicable. Missing upstream or local-only/
diverged commits produce unmet work, not a reset or rebase. Other branch-query
errors remain failures.

Apply fetches and uses `git merge --ff-only @{u}`. A successfully changed
checkout triggers the guarded [configuration restart](architecture.md#repository-updates-are-a-restart-boundary)
before later tasks run. The parent keeps the run lock. Updating two repositories
is not a transaction: a later failure does not undo an earlier success.

Dry-run may query remote refs with `git ls-remote`, but does not fetch or merge.
For update, `--no-repo-update` removes this task even from dependency expansion.
It does **not** disable binary self-update or all network access.

### Git hooks

Copies extensionless files from `hooks/` after repository synchronization,
honoring `core.hooksPath` and the common hooks directory of linked worktrees.
When the destination is already the tracked source, it is neither copied over
itself nor removed.

An existing hook with a managed name can be overwritten. Dotfiles neither
combines it with the new hook nor backs it up, so save custom hooks first.
[Hooks](contributing.md#git-hooks) explains what the installed hooks do.

### Shell completions

Writes completion registration after repository synchronization:

- Linux: `symlinks/config/zsh/completions/_dotfiles` **inside the checkout**.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Candidates come from the running CLI, including configuration-aware selectors.
This task does not install the shell or all its profile links.

### Global Git settings

Applies `conf/git-config.toml` values to **global** Git configuration.
Repeated occurrences of managed keys are handled without altering unrelated
keys. Profile/overlay persistence instead uses repository-local configuration.

On Windows, an undeclared global `core.autocrlf` is removed, even with no
other configured Git settings. An explicit active declaration overrides that
cleanup. On other platforms, empty settings mean no work.

### Agent settings

Applies selected dot-separated keys to Copilot JSON and Codex TOML documents,
preserving undeclared and harness-owned keys. It does not deploy skills/plugins.
See [Agent harness settings](configuration-reference.md#agent-harness-settings).

### System configuration files

On Linux outside CI, merges declared `system/` fragments into files below
`/etc`. TOML, INI and PAM have different ownership rules; review the
[configuration format](configuration-reference.md#system-files) before editing.

Malformed or non-regular destinations fail. Missing PAM service files are not
created. Changed content is staged and installed as `root:root`, mode `0644`,
using `sudo install`; dry-run does not write those destinations.

### System packages

Installs missing non-AUR declarations using pacman on Linux or winget on
Windows. There is no apt/dnf adapter. A missing package manager leaves the
task skipped; a failed installed-package query fails the task.
A failed batch transaction rechecks state to retain partial progress without
turning the transaction into success.

**Installing missing Arch packages can upgrade the whole system.**
The command is `pacman -Syu --needed --noconfirm`. If all declared packages are already present, this
task does not run an upgrade solely because the command is named update.

Windows installs missing IDs through winget. Installer failures fail the task,
even if other packages succeed; installers can request their own UAC consent.
See [Windows package behavior](package-management.md#windows) for user-scope
fallback and failure handling, and [Windows elevation](windows-and-wsl.md#elevation)
before an elevated retry.

### Paru package manager

On pacman platforms, checks both a registered `paru` package and a working
`/usr/bin/paru --version`. It can rebuild an installed helper broken by a
library upgrade. Failed package queries or interrupted health probes fail
rather than trigger a speculative rebuild.

It waits for system packages without propagating unrelated failures. A rebuild
needs `git`, `makepkg`, `sudo` and a working `cargo --version`; an unconfigured
rustup proxy is not enough. The rebuilt helper is checked again before
dependent AUR installation.

Arch provisioning probes the target chroot, not the live ISO. AUR work uses
the validated `/usr/bin/paru`, not an arbitrary PATH copy.

### AUR packages

Installs missing `aur = true` entries after successful Paru bootstrap. It also
waits for custom package repository preparation without making recipe failures block every
unrelated AUR package.

Preview queries pacman's installed-package database without requiring the
helper that bootstrap has only planned to install. Query errors still fail
the preview. Applying this task builds and executes package-supplied code.

### Home symlinks

Links sources from the declaring repository's `symlinks/` into home.
Overlay entries retain their origin.

**Existing regular files and empty directories can be replaced without backup,
replacement warning or confirmation.** Nonempty directories fail rather than
being recursively deleted. Move unrelated targets aside before applying.

Linked sources stay live. On Windows, the task independently assesses
elevation and rejects Git symlink placeholders checked out as ordinary files.
See [Symlinks](configuration-reference.md#symlinks) for path and glob rules.

### File permissions

After symlink success, applies Unix modes to existing configured home targets.
Directories recurse and retain traversal bits; ordinary files have execute
bits cleared unless explicitly configured otherwise. Missing content is not
created. Changes through links can affect the checkout.

### Login shell

On Linux outside CI, sets the account login shell to zsh. This choice is
task-defined, not configurable through TOML. It waits for packages and reports
missing zsh as unmet work.

State comes from the account database, not `$SHELL`. Apply uses `usermod` as
root, `sudo -n usermod` when available without prompting, otherwise `chsh`.
A matching shell name with a missing/unusable executable still needs repair.
Your currently running terminal shell is not replaced.

### Systemd units

Enables/starts enabled declarations and disables/stops disabled ones. It waits
for regular/AUR packages, symlinks and permissions using ordering-only edges,
then inspects actual unit state. This task never installs packages itself.

It needs systemctl, is excluded in CI, and inside WSL requires systemd already
running or degraded. System units can require sudo. Without a user manager,
including Arch chroot provisioning, it creates per-user enablement links for
the next real login instead of running services inside the chroot.
See [Unit configuration](configuration-reference.md#systemd-units).

### User registry settings

Applies current-user values, preserving undeclared ones. This is separate
from machine-level Developer Mode and does not request brokered elevation.
Some applications read these settings only at startup. Remove does not restore
former values.

### VS Code extensions

After waiting for package tasks, finds an editor CLI, reads installed
extensions, and installs missing ones. If no CLI is available, the task is
skipped rather than reported as successful.

Arch chroot provisioning instead defers installation to
`dotfiles-first-login.service`, which retries until configured extensions
are installed in a real user session.

### Agent packages

Updates agent dependencies within their declared version rules and deploys
content for the current user from selected main/overlay APM fragments. It
requires successful symlink work and normally waits for package/AUR tasks.
Readiness checks can allow it to proceed before those package tasks finish.

See [Agent configuration](agent-configuration.md) for sources, targets,
authentication, and cleanup, including
[what a preview can show](agent-configuration.md#what-a-preview-can-show).
The [implementation reference](architecture.md#agent-deployment-implementation)
explains the command sequence and compatibility adapters.

### Custom Arch packages

`custom-package-versions` discovers stable release pins. `custom-package-repository` prepares
disposable Git snapshots and configures Paru to use them. They are ordered
without making a recipe failure block unrelated AUR work.

Refresh failures still count as failures, although existing pins can produce
a usable snapshot. Unavailable custom packages are excluded from direct
install targets and counted as failures when missing. Descriptor names
establish dotfiles ownership: failure must not silently fall back to the AUR.

`CleanAfter` cleans the cache, not this checkout. See
[Package management](package-management.md#custom-arch-packages) for the current
packages and [Arch package recipes](package-management.md#arch-package-recipes) for version
discovery, snapshot ownership, and adding recipes.

## Dynamic overlay tasks

Each active entry in an overlay's `conf/scripts.toml` becomes a separate task:

- Display label: the configured name.
- Selector: `script-<normalized-name>`.
- Commands: update and remove.
- Output: its own result and captured script output.

These instances are discovered at startup and rediscovered after a repository
restart, not added during task execution.
An internal reporting task records their count but does not execute scripts.

| Script invocation | Behavior |
|---|---|
| `--check` | Exit 0 when configured; exit 1 when work is needed |
| No flag | Apply when the update check returns 1 |
| `--dryrun` | Describe the changes needed without applying them |
| `--remove` | Remove when the removal check returns 0 |

A missing/unreadable script or another check failure fails the task. A remove
dry run inspects state and reports removal, invoking neither `--remove` nor
`--dryrun`. Script safety is cooperative: even preview executes script code.
See [Overlay scripts](private-overlays.md#overlay-scripts) for the full protocol.

## Remove tasks

Remove uses **current configuration, not a historical inventory**. Keep the
original profile, overlay and sources available until it completes.

| Selector | What removal does |
|---|---|
| `symlinks` | Replace managed links with source copies; also fill absent targets. Preserve existing non-link targets. |
| `git-hooks` | Delete matching managed hooks; preserve modified hooks and unrelated names. |
| `launcher` | Delete a matching managed launcher; preserve modified launchers and leave PATH unchanged. |
| `script-…` | Run the active script's removal protocol. Its safety and completeness depend on the script. |

This is not restoration from backups. Packages, services, registry/Git/harness
values, shell selection, permissions, completions, WSL settings, editor
extensions and agent deployments are not reversed by static remove.

Materialized directory trees preserve nested symlinks rather than copying
everything they reference. Ownership is rechecked before replacing a managed
link, so a link retargeted since inspection is preserved. File copies are
staged privately on Unix and receive source permissions before publication.

## Validation tasks

`check` has its own task set; it does not execute the update graph.

| Selector | What it checks |
|---|---|
| `config-diagnostics` | Aggregated diagnostics; warning-severity findings also fail |
| `symlink-sources` | Symlink/glob and chmod sources, including inactive categories |
| `config-files` | Required main TOML files; warns if `hooks/` is absent |
| `apm-plugins` | Local plugins through `apm pack --dry-run --verbose` |
| `shellcheck` | Shell scripts in the main repository |
| `psscriptanalyzer` | PowerShell scripts in the main repository |

All commands load configuration before filtering. `--only` cannot bypass a
malformed [required file](configuration-reference.md#files).

Missing optional tools cause visible unmet skips; `--fail-on-skip` or CI makes
these unsuccessful. Local plugin checks do not validate every remote package
or private overlay script.

Use [CLI validation](testing.md#cli-validation) for commands, then select
additional integration/configuration-drift coverage as appropriate.

## Scheduling model

Tasks run when their dependencies allow, not in catalog order.

| Dependency | Meaning when both tasks are selected |
|---|---|
| `requires <-` (`blocking` in JSON) | Predecessor failure or unmet work blocks the dependent |
| `after <-` (`after` in JSON) | Wait, then inspect this task even if the predecessor failed |

For example, AUR installation requires a working Paru bootstrap. Systemd only
waits for package and symlink work: an unrelated failure there must not prevent
it inspecting units that are available.

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
