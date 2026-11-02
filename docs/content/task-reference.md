---
title: Task reference
description: Find task selectors, dependencies, managed state, and the boundaries of update, remove, and validation tasks.
---

# Task reference

A **task selector** chooses a kind of work, such as `symlinks` or `packages`.
It does not choose one file, package or service inside that task. Entries come
from the selected [configuration](configuration-reference.md), profile and overlay.

Start with read-only discovery using your configured checkout and profile.
Pass `--profile base` or `--profile desktop` if you have not selected one yet:

```bash
dotfiles list
dotfiles list --all
dotfiles list --graph update
```

Ordinary discovery hides public tasks that clearly have no platform/configured
work. `--all` shows the complete public catalog. Neither checks runtime
prerequisites or changes machine state, so a listed task may still find its
tool unavailable when executed.

The table groups selectors alphabetically by command: update, remove, check.
Plain/JSON listings contain each selector once with its command membership.
Graphs retain all nodes, including filtered and internal nodes, and cannot be
combined with `--all`.

For command syntax and preview side effects, see [Usage](commands-and-usage.md#select-tasks).

## Scheduling model

Tasks run when their dependencies allow, not in catalog order.

| Dependency | Meaning when both tasks are selected |
|---|---|
| `requires <-` (`blocking` in JSON) | Predecessor failure or unmet work blocks the dependent |
| `after <-` (`after` in JSON) | Wait, then inspect this task even if the predecessor failed |

For example, AUR installation requires a working Paru bootstrap. Systemd only
waits for package and symlink work: an unrelated failure there must not prevent
it inspecting units that are available.

Selection is explicit:

- `--only` matches exact normalized selectors or full labels; it does not
  automatically add prerequisites.
- Filtering out a blocking prerequisite warns and assumes it is satisfied.
- On update, `--with-deps` recursively includes both kinds of predecessor.
- `--skip` can remove those predecessors again.
- Unknown selectors and explicitly empty selections are errors.

Listings and completions use canonical selectors. For compatibility, `apm`,
`paru-config`, `shell` and `config-warnings` still select `agent-packages`,
`local-package-repository`, `login-shell` and `config-diagnostics`, respectively.
Previous full console labels remain accepted by `--only` and `--skip` too.

Inspect expansion before applying:

```bash
dotfiles list --graph update --only systemd --with-deps
```

The default graph format uses one block per task, with predecessors first.
Selection markers are `default`, `requested`, `dependency`, `filtered` and
`skipped`. Internal orchestration is marked `internal`; it is not selectable
and stays out of ordinary task rows and totals. Plain output uses tab-separated
rows; JSON supplies structured edges and selection fields.

This is a **selection graph, not a change plan**. Use `--skip repository` to
exclude repository work from a listing; `list` does not accept
`--no-repo-update`.

### Conditional waits

VS Code, Agent packages and systemd may run before their package predecessors
when a once-per-phase assessment proves that:

1. The consumer's required inputs are ready.
2. On a pacman platform, every configured package is installed and Paru is
   healthy, so no configured install or helper rebuild will invalidate them.

Missing inputs, unknown/failed probes or other package managers retain the
waits. Systemd uses this shortcut only for already-converged units and still
waits for symlinks/permissions. Agent packages retains its blocking symlink
prerequisite. Execution rechecks inputs; blocking edges are never waived.
The declared listing and `--with-deps` expansion remain unchanged.

Independent ready tasks may run concurrently, with console rows in completion
order. `--no-parallel` disables parallel execution. Tasks may finish current,
not applicable or unmet rather than changed; failure is not a rollback.

## Installation tasks

### Catalog overview

These are all static public **update** selectors. Follow a label for its
details. Availability describes broad eligibility, not a guarantee that
runtime prerequisites or elevation are available. Only `git-hooks`, `launcher`
and `symlinks` also belong to [remove](#remove-tasks).

| Selector | Console label and availability |
|---|---|
| `agent-packages` | [Agent packages](#agent-packages): active APM fragments |
| `agent-settings` | [Agent settings](#agent-settings): configured harness settings |
| `aur-packages` | [AUR packages](#aur-packages): Arch/AUR support and AUR entries |
| `completions` | [Shell completions](#shell-completions): supported shell registration |
| `developer-mode` | [Windows Developer Mode](#windows-developer-mode): Windows |
| `file-permissions` | [File permissions](#file-permissions): supported Unix modes and configured paths |
| `git` | [Global Git settings](#global-git-settings): configured values or Windows autocrlf cleanup |
| `git-hooks` | [Git hooks](#git-hooks): repository hook sources |
| `launcher` | [Dotfiles launcher](#dotfiles-launcher): Linux or Windows |
| `local-package-repository` | [Local package repository](#local-pkgbuild-packages): Arch/AUR support |
| `local-package-versions` | [Local package updates](#local-pkgbuild-packages): Arch/AUR support and local recipes |
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

`local-package-versions` is explicitly update-only metadata: it advances local
recipe release pins. That is not an ordering tier or a remove capability.

### Host capability and wrapper tasks

#### Windows Developer Mode

Enables the machine-level Developer Mode policy when unset, requiring
Administrator rights. Home symlinks waits with an ordering-only edge, so
unavailable Developer Mode does not block links that need no change or
directory junctions that can be created without it.

Pending file symlinks may still need their own elevation. The CLI normally
stays unelevated and delegates only affected tasks.
See [Windows elevation](windows-and-wsl.md#elevation).

#### Dotfiles launcher

Writes a shim that delegates to this checkout's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

It repairs stale content; it does not copy the Rust binary. Moving the checkout
can invalidate the recorded path.

#### Shell PATH

Requires the launcher. Adds `~/.local/bin` through a managed export in
`~/.profile` on Linux, or the user PATH on Windows. Open a new shell to use
the persisted change.

Linux comments or example text do not satisfy the managed export check.
Windows preserves registry value type and expandable tokens, and fails without
rewriting unsupported types, malformed UTF-16 or embedded NULs. Remove leaves
the PATH addition in place.

### Repository and source tasks

#### Dotfiles repository

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

#### Git hooks

Copies extensionless files from `hooks/` after repository synchronization,
honoring `core.hooksPath` and the common hooks directory of linked worktrees.
When the destination is already the tracked source, it is neither copied over
itself nor removed.

Existing hooks at managed names can be replaced without composition or backup.
Save custom hooks first. [Hooks](git-hooks.md) explains what the installed hooks do.

#### Shell completions

Writes completion registration after repository synchronization:

- Linux: `symlinks/config/zsh/completions/_dotfiles` **inside the checkout**.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Candidates come from the running CLI, including configuration-aware selectors.
This task does not install the shell or all its profile links.

#### Report overlay scripts

An internal task records the number of active overlay script definitions.
It is not a public selector and does not execute those scripts; each script
gets its own [dynamic task](#dynamic-overlay-tasks).

### System convergence tasks

#### Global Git settings

Converges `conf/git-config.toml` values in **global** Git configuration.
Repeated occurrences of managed keys are handled without altering unrelated
keys. Profile/overlay persistence instead uses repository-local configuration.

On Windows, an undeclared global `core.autocrlf` is removed, even with no
other configured Git settings. An explicit active declaration overrides that
cleanup. On other platforms, empty settings mean no work.

#### Agent settings

Converges selected dot-separated keys in Copilot JSON and Codex TOML documents,
preserving undeclared and harness-owned keys. It does not deploy skills/plugins.
See [Agent harness settings](configuration-reference.md#agent-harness-settings).

#### System configuration files

On Linux outside CI, merges declared `system/` fragments into files below
`/etc`. TOML, INI and PAM have different ownership rules; review the
[configuration format](configuration-reference.md#system-files) before editing.

Malformed or non-regular destinations fail. Missing PAM service files are not
created. Changed content is staged and installed as `root:root`, mode `0644`,
using `sudo install`; dry-run does not write those destinations.

#### System packages

Installs missing non-AUR declarations using pacman on Linux or winget on
Windows. There is no apt/dnf adapter. Missing package-manager tools are unmet
prerequisites; failed inventory queries are errors, not empty inventories.
A failed batch transaction rechecks state to retain partial progress without
turning the transaction into success.

**Missing Arch packages invoke `pacman -Syu --needed --noconfirm`.** This can
upgrade the wider system. If all declared packages are already present, this
task does not run an upgrade solely because the command is `update`.

Windows tries winget with `--scope user`, then retries without scope only for
“no applicable installer.” The task does not broker elevation, but an installer
may request UAC. Administrator-required, declined or policy-blocked installs
are recorded as **failed package items**; other installs can continue, but the
command fails even without `--fail-on-skip`. These are not harmless missing-tool
skips. See [Windows packages](windows-and-wsl.md#packages).

#### Paru package manager

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

#### AUR packages

Installs missing `aur = true` entries after successful Paru bootstrap. It also
waits for local package repository preparation without making recipe failures block every
unrelated AUR package.

Preview queries pacman's installed-package database without requiring the
helper that bootstrap has only planned to install. Query errors still fail
the preview. Applying this task builds and executes package-supplied code.

#### Home symlinks

Links sources from the declaring repository's `symlinks/` into home.
Overlay entries retain their origin.

**Existing regular files and empty directories can be replaced without backup,
replacement warning or confirmation.** Nonempty directories fail rather than
being recursively deleted. Move unrelated targets aside before applying.

Linked sources stay live. On Windows, the task independently assesses
elevation and rejects Git symlink placeholders checked out as ordinary files.
See [Symlinks](configuration-reference.md#symlinks) for path and glob rules.

#### File permissions

After symlink success, applies Unix modes to existing configured home targets.
Directories recurse and retain traversal bits; ordinary files have execute
bits cleared unless explicitly configured otherwise. Missing content is not
created. Changes through links can affect the checkout.

#### Login shell

On Linux outside CI, sets the account login shell to **zsh**. This choice is
task-defined, not configurable through TOML. It waits for packages and reports
missing zsh as unmet work.

State comes from the account database, not `$SHELL`. Apply uses `usermod` as
root, `sudo -n usermod` when available without prompting, otherwise `chsh`.
A matching shell name with a missing/unusable executable still needs repair.
Your currently running terminal shell is not replaced.

#### Systemd units

Enables/starts enabled declarations and disables/stops disabled ones. It waits
for regular/AUR packages, symlinks and permissions using ordering-only edges,
then inspects actual unit state. The [conditional waits](#conditional-waits)
can avoid unnecessary package waits; this task never installs packages itself.

It needs systemctl, is excluded in CI, and inside WSL requires systemd already
running or degraded. System units can require sudo. Without a user manager,
including Arch chroot provisioning, it creates per-user enablement links for
the next real login instead of running services inside the chroot.
See [Unit configuration](configuration-reference.md#systemd-units).

#### User registry settings

Converges current-user values, preserving undeclared ones. This is separate
from machine-level Developer Mode and does not request brokered elevation.
Some applications read these settings only at startup. Remove does not restore
former values.

#### VS Code extensions

Finds an available editor CLI, queries installed extensions and installs
missing ones after its package waits. A missing CLI is unmet work, not success.

Arch chroot provisioning instead defers installation to
`dotfiles-first-login.service`, which retries until configured extensions
converge in a real user session.

#### Agent packages

Builds user-scope desired state from active main/overlay APM fragments.
Symlink success is blocking; package/AUR work is ordering-only. Conditional
readiness can waive the package waits, never the symlink prerequisite.

Update runs `apm update -g --yes`, then `apm install -g` to reconcile local
content and deployment drift. Preview uses `apm update -g --dry-run` only when
the generated manifest is already current; it describes, but does not run,
the follow-up install. When a new manifest is needed, preview reports the
write/delegated work rather than planning against stale input.

See [AI agent configuration](ai-agent-configuration.md) for target integrations, generated-state
ownership, authentication and cleanup. This task is broader than copying skills.

### Local PKGBUILD packages

`local-package-versions` discovers stable release pins. `local-package-repository` prepares
disposable Git snapshots and configures Paru to use them. They are ordered
without making a recipe failure block unrelated AUR work.

Refresh failures still count as failures, although existing pins can produce
a usable snapshot. Unavailable local packages are excluded from direct
install targets and counted as failures when missing. Descriptor names
establish local ownership: failure must not silently fall back to the AUR.

`CleanAfter` cleans the cache, not this checkout. See
[Package management](package-management.md#local-arch-packages) for the current recipes, version
discovery, snapshot ownership and adding packages.

## Dynamic overlay tasks

Each active entry in an overlay's `conf/scripts.toml` becomes a separate task:

- Display label: the configured name.
- Selector: `script-<normalized-name>`.
- Commands: update and remove.
- Output: its own result and captured script output.

These instances are discovered at startup and rediscovered after a repository
restart, not added during task execution.

| Mode | Invocation | Behavior |
|---|---|---|
| Inspect | `--check` | Exit 0: configured state present; exit 1: absent/needs work |
| Apply | No flag | Apply when the update check returns 1 |
| Preview | `--dryrun` | Preview needed application without mutation |
| Remove | `--remove` | Remove when the removal check returns 0 |

A missing/unreadable script or another check failure fails the task. A remove
dry run inspects state and reports removal, invoking neither `--remove` nor
`--dryrun`. Script safety is cooperative: even preview executes script code.
See [Overlay scripts](private-overlays.md#overlay-scripts) for the full protocol.

## Remove tasks

Remove uses **current configuration, not a historical inventory**. Keep the
original profile, overlay and sources available until it completes.

| Selector | Removal | Limit |
|---|---|---|
| `symlinks` | Materialize configured source content as local copies | Also fills absent targets; preserves existing non-link targets |
| `git-hooks` | Delete installed hooks matching managed state | Preserve modified hooks and unrelated names |
| `launcher` | Delete the launcher matching managed content/state | Preserve modified launchers and leave PATH unchanged |
| `script-…` | Invoke active scripts' removal protocol | Only as safe/complete as the script |

This is not restoration from backups. Packages, services, registry/Git/harness
values, shell selection, permissions, completions, WSL settings, editor
extensions and agent deployments are not reversed by static remove.

Materialized directory trees preserve nested symlinks rather than copying
everything they reference. Ownership is rechecked before replacing a managed
link, so a link retargeted since inspection is preserved. File copies are
staged privately on Unix and receive source permissions before publication.

## Validation tasks

`check` has its own task set; it does not execute the update graph.

| Selector | Console label | Scope |
|---|---|---|
| `config-diagnostics` | Configuration diagnostics | Fail on aggregated diagnostics, including warning-severity findings |
| `symlink-sources` | Validate symlink sources | Symlink/glob and chmod sources, including inactive categories |
| `config-files` | Validate config files | Required main TOML inventory; warn if `hooks/` is absent |
| `apm-plugins` | Validate APM plugins | Native `apm pack --dry-run --verbose` for main-checkout plugins |
| `shellcheck` | Shellcheck | Shell scripts discovered in the main repository |
| `psscriptanalyzer` | PSScriptAnalyzer | PowerShell scripts discovered in the main repository |

All commands load configuration before filtering. `--only` cannot bypass a
malformed [required file](configuration-reference.md#files).

Missing `apm`, `shellcheck`, `sh` or `pwsh` causes a visible unmet skip;
`--fail-on-skip` or CI makes these unsuccessful. If `pwsh` exists but the
PSScriptAnalyzer module does not, the analyzer fails. Local plugin checks
do not validate every remote package or private overlay script.

Use [CLI validation](testing.md#cli-validation) for commands, then select
additional integration/configuration-drift coverage as appropriate.

## Filtering examples

These examples inspect or preview rather than apply. Use the intended checkout:

```bash
# Just home links; omitted prerequisites are assumed satisfied.
dotfiles update --only symlinks --dry-run

# Inspect everything dependency expansion would add.
dotfiles list --graph update --only agent-packages --with-deps

# Preview regular packages and APM, without adding AUR tasks.
dotfiles update --only packages,agent-packages --dry-run
```

For scripts, select the real overlay, discover its selector with `list`, then
use that exact value. Previewing it still executes its inspection/preview code.
