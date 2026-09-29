# Task reference

Use a **selector** to run one operation against the active configuration.
Selectors do not select individual packages, files, or services.
From the repository root, with an existing CLI on PATH:

```bash
dotfiles tasks --root . --profile desktop
dotfiles tasks --root . --profile desktop --graph install
```

Discovery loads the selected profile and overlay but does not probe machine
state or execute tasks. A task appearing here may be inapplicable on your host.
For command syntax, selection rules, and preview side effects, see
[Usage](USAGE.md#select-tasks).

## Scheduling model

Tasks are connected by two kinds of dependencies:

| Graph column | Meaning when both tasks are selected |
|---|---|
| `BLOCKING` | The predecessor must succeed; failure or unmet work blocks its dependents |
| `AFTER` | Wait for the predecessor, but still check this task if it fails |

For example, AUR installation requires a healthy Paru bootstrap. Systemd waits
for package, AUR, symlink, and permission work, but unrelated failures there do
not stop it inspecting each unit.

`--only` selects exact normalized selectors or full labels; it does not
automatically select prerequisites. Filtering out a blocking prerequisite
warns and assumes it is satisfied. On install/update, `--with-deps` recursively
includes both kinds of predecessor. `--skip` can remove them again.
Unknown selectors and explicit empty selections are errors.

Inspect expansion before applying it:

```bash
dotfiles tasks --root . --profile desktop --graph install \
  --only systemd --with-deps --format json
```

Graph output retains filtered nodes and edges and marks their `SELECTION`.
It also includes internal orchestration (`INTERNAL`), hidden from normal
discovery and task totals. It is a dependency/selection view, not a machine
change plan. For a graph without repository work, use `--skip repository`;
`tasks` does not accept `--no-repo-update`.

Independent ready tasks can run concurrently, and console rows appear in
completion order. `--no-parallel` disables parallel execution; catalog order
is not an execution-order contract. Membership in `update` likewise does not
imply ordering.

Built-in mutation tasks inspect current state and support CLI dry-run planning.
Tasks can finish current, not applicable, or skipped rather than changed.
External overlay scripts must implement their own safety contract.

## Installation tasks

### Catalog overview

All rows below belong to both **install** and **update**. The APM task changes
mode for updates. Rows marked “yes” also belong to uninstall; no other static
install task is reversed by uninstall.

| Selector | Console label | Inputs / affected state | Uninstall |
|---|---|---|---|
| `developer-mode` | Windows Developer Mode | Windows machine symlink capability | — |
| `repository` | Dotfiles repository | Main checkout and Git overlay | — |
| `git` | Git settings | `conf/git-config.toml` → global Git config | — |
| `agent-settings` | Agent settings | `conf/agent-settings.toml` → harness settings | — |
| `git-hooks` | Git hooks | `hooks/` → Git's hooks directory | yes |
| `completions` | Shell completions | Generated Zsh/PowerShell registration | — |
| `packages` | System packages | Non-AUR entries in `conf/packages.toml` | — |
| `paru` | Paru package manager | Arch's `/usr/bin/paru` | — |
| `aur-packages` | AUR packages | Entries marked `aur = true` in `conf/packages.toml` | — |
| `symlinks` | Home symlinks | `conf/symlinks.toml` → home links | yes |
| `file-permissions` | File permissions | `conf/chmod.toml` → home target modes | — |
| `shell` | Default shell | Linux account login shell → zsh | — |
| `system-files` | System files | `conf/system-files.toml` + `system/` → `/etc` | — |
| `systemd` | Systemd units | `conf/systemd-units.toml` → unit enablement/startup | — |
| `registry` | Windows registry | `conf/registry.toml` → current-user values | — |
| `vscode-extensions` | VS Code extensions | `conf/vscode-extensions.toml` → editor extensions | — |
| `apm` | APM packages | Active APM fragments → generated user-scope deployment | — |
| `launcher` | Dotfiles launcher | Generated shim in `~/.local/bin` | yes |
| `path` | Shell PATH | `~/.local/bin` in the user's PATH | — |

### Host capability and wrapper tasks

#### Windows Developer Mode

Enables Developer Mode when the Windows policy value is unset. This
machine-level registry mutation needs Administrator rights. **Home symlinks**
has a blocking dependency on it; normal runs remain unelevated and delegate
only tasks needing elevation.

Pending file links can themselves require elevation when Developer Mode is off.
Directory links can fall back to junctions. Unavailable elevation skips
affected tasks and blocks their dependents, not the whole independent graph.
See [Windows elevation](WINDOWS.md#elevation).

#### Dotfiles launcher

Writes a small launcher that delegates to the repository's wrapper:

- Linux: `~/.local/bin/dotfiles`.
- Windows: `%USERPROFILE%\.local\bin\dotfiles.cmd`, preferring `pwsh` and
  falling back to Windows PowerShell.

It replaces stale launcher content. This is not a copy of the Rust binary,
and moving the checkout can invalidate the saved path.

#### Shell PATH

Requires **Dotfiles launcher**. On Linux, persists an export in `~/.profile`
when needed. On Windows, updates the user PATH while preserving registry value
type and expandable tokens. Start a new shell before relying on the change.
Uninstall leaves the PATH addition in place.

### Repository and source tasks

#### Dotfiles repository

Both install and update consider the main checkout and an overlay with a
`.git` entry. Tracked local changes prevent synchronization; untracked files
are ignored by that readiness check. Detached HEAD is inapplicable. Missing
upstream or local-only/diverged commits are reported as unmet work rather than
being reset or rebased.

Apply fetches and uses `git merge --ff-only @{u}`. A changed checkout triggers
a guarded child with the original arguments, which reloads configuration and
rediscovers tasks before proceeding. The parent retains the repository lock.
The main and overlay updates are not a cross-repository transaction; a later
failure does not roll back an earlier successful update.

Dry-run may query remote refs with `git ls-remote`; it does not fetch or merge.
`--no-repo-update` removes this task from install/update, including dependency
expansion, and keeps the current checkout as the source. It does not disable
binary self-update or other network access.

#### Git hooks

Copies extensionless hook files from `hooks/`, after repository synchronization.
Uses the common Git hooks directory for linked worktrees and honors
`core.hooksPath`. If that path already points to the tracked sources, they are
not copied over themselves or removed.

Existing hook content at managed destination names can be replaced; there is
no hook-composition or backup mechanism. Save custom hooks before converging.
See [Hooks](HOOKS.md) for their behavior.

#### Shell completions

After repository synchronization, writes runtime completion registration:

- Linux: `symlinks/config/zsh/completions/_dotfiles` inside the checkout.
- Windows: `~/.config/powershell/profile.d/dotfiles-completions.ps1`.

Completion candidates come from the running CLI, including configuration-aware
task selectors. This task does not itself install the shell or all its profile
links.

#### Report overlay scripts

This is internal bookkeeping, not a selectable operation. It reports the
number of active script definitions in the run log. Actual script execution
belongs to the separate [dynamic tasks](#dynamic-overlay-tasks).

### System convergence tasks

#### Git settings

Converges active declared values in **global** Git configuration, not just the
checkout's `.git/config`. Empty configuration produces no work. Profile and
overlay selection persistence, by contrast, uses local Git configuration.

#### Agent settings

Converges declared dot-separated keys in Copilot JSON and Codex TOML settings,
preserving undeclared and volatile harness-owned keys. This task is distinct
from deploying plugins/skills through APM.
See [Agent harness settings](CONFIGURATION.md#agent-harness-settings).

#### System files

On Linux outside CI, merges selected tracked fragments into administrator-owned
files below `/etc`. The current configuration covers Codex requirements on
Linux, pacman options on Arch, GNOME Keyring PAM integration for Arch desktop,
and WSL configuration inside WSL.

TOML, INI, and PAM use different merge strategies that preserve unmanaged
content. Malformed/non-regular targets fail; absent PAM service files are not
created. Changed content is staged and installed as `root:root`, mode `0644`,
using `sudo install`. Dry-run does not write these targets.
See [System files](CONFIGURATION.md#system-files) before changing fragments.

#### System packages

Installs missing non-AUR entries through pacman on Linux or winget on Windows.
The Linux configuration is intended for Arch; there is no apt/dnf adapter.
Missing pacman/winget is unmet work. Installed-state query failures are errors,
not an empty package inventory.

**On Arch, installing missing packages invokes
`pacman -Syu --needed --noconfirm`.** This can update the wider system as well
as install the missing entries; it is not an isolated file-copy operation.
If every configured package is already present, this task does not run a
package upgrade solely because the command was `update`.

On Windows, winget is tried with `--scope user`, then without a scope only for
“no applicable installer.” The task does not broker elevation, but an installer
can request UAC. Administrator-required, declined, or policy-blocked installs
are reported as unmet/skipped work. See [Windows packages](WINDOWS.md#packages).

#### Paru package manager

On Arch, ensures the target system has both a registered `paru` package and a
working `/usr/bin/paru --version`. It can rebuild an installed helper after a
library upgrade makes it unusable.

The task waits for system packages using an ordering-only edge. Before cloning
or building AUR source it requires `git`, `makepkg`, `sudo`, and a working
`cargo --version`; an unconfigured rustup proxy is not sufficient. The completed
helper is checked again before dependent AUR work runs.

Under `install-arch`, checks run inside the target chroot, not against the live
ISO. Later AUR operations use validated `/usr/bin/paru`, not a stale PATH copy.

#### AUR packages

Installs missing AUR entries after **Paru package manager** succeeds. Dry-run
queries pacman's installed-package database without requiring the helper that
bootstrap only planned to install. A failed database query still fails the
preview. Installing AUR packages builds and executes package-supplied code.

#### Home symlinks

Expands configured sources from the main or overlay `symlinks/` tree into
home-relative links. Source provenance is retained; overlay entries do not
silently become main-repository paths.

**Existing regular files and empty directories at managed targets can be
replaced without backup.** The preview warns about non-link targets; apply
does not ask for confirmation. Nonempty directories fail rather than being
recursively deleted. Back up or relocate unrelated targets before applying.

Links keep the checkout live: applications can see tracked source edits
immediately. On Windows, the task depends on Developer Mode and rejects Git
symlink placeholders checked out as ordinary files.
See [Symlinks](CONFIGURATION.md#symlinks) for target and glob rules.

#### File permissions

After symlink success, applies declared Unix modes to existing home targets.
Directory entries recurse, preserve directory traversal bits, and clear
ordinary-file execute bits; explicit file entries can declare executable
modes. On linked targets, permission changes can affect the source checkout.
The task does not create missing target content.

#### Default shell

On Linux outside CI, sets the account's login shell to **zsh**. This is
task-defined, not a shell name selected from TOML. It waits for system
packages, then reports missing zsh as unmet work.

State comes from the account database, not `$SHELL`. Apply uses `usermod` as
root, `sudo -n usermod` when cached/passwordless sudo works, otherwise `chsh`.
This does not replace the shell process already running in your terminal.

#### Systemd units

Converges enablement and runtime state: enabled entries are enabled/started,
and `enabled = false` entries are disabled/stopped. Bare names default to
enabled user units. System-scope units can require sudo. It waits for package, AUR, symlink, and
permission tasks via ordering-only edges, then inspects actual unit state.

The task requires systemctl and is excluded in CI. Inside WSL, systemd must
already be running or degraded; editing `wsl.conf` does not start it in the
current distribution session.

Without a user manager, including Arch chroot provisioning, it creates
per-user enablement links offline and leaves startup to a real login. An
“enabled” result is not a promise that a service is running in the chroot.

#### Windows registry

Converges declared current-user values without deleting undeclared values.
This task is separate from machine-level Developer Mode and does not request
brokered elevation. Some settings are only read at application/session startup.
Uninstall does not restore their former values.

#### VS Code extensions

After regular/AUR packages, finds an available VS Code CLI, queries installed
extensions, and installs missing entries. A missing editor CLI is unmet work,
not success.

Arch chroot provisioning defers installation to `dotfiles-first-login.service`
instead of attempting Marketplace installation without a real user session.
The first-login work is retried until the configured extensions converge.

#### APM packages

Builds user-scope desired state from active repository/overlay APM fragments.
Symlink success is blocking; regular/AUR package work is ordering-only so APM
availability can be rechecked after package installation.

| Command mode | Native convergence |
|---|---|
| Ordinary install | `apm install -g` |
| update / install `--update` | `apm update -g --yes`, without a preceding install pass |
| Update preview with current generated manifest | `apm update -g --dry-run` |
| Update preview needing a new generated manifest | Reports manifest write and delegated update; does not plan against stale input |

This task also manages target-specific integration; it is broader than copying
skill files. See [APM](APM.md) for generated state ownership, supported targets,
authentication, and cleanup. Do not edit generated outputs as source.

## Dynamic overlay tasks

Only an overlay can provide `conf/scripts.toml`. Each active script gets:

- Its configured `name` as the display label.
- A stable `script-<normalized-name>` selector.
- Install, update, and uninstall membership.
- A separate task result and captured output.

Discovery happens during configuration startup and repeats in a child after
repository synchronization.

| Mode | Script invocation | Contract |
|---|---|---|
| Check | `--check` | Exit 0: desired/managed state present; exit 1: work needed/state absent |
| Apply | No flag | Apply desired state when the install check returns 1 |
| Preview | `--dryrun` | Preview needed install work without mutation |
| Remove | `--remove` | Remove state on uninstall when the check returns 0 |

A missing script or other check exit fails the task. Uninstall dry-run still
checks state but does not execute `--remove`.
The engine cannot prevent side effects from a script that violates check or
preview mode. Review it before running even a dry run.
See [Overlay scripts](CONFIGURATION.md#overlay-scripts) for authoring rules.

## Uninstall tasks

Uninstall uses **the currently selected configuration**, not a historical
inventory. Keep the original profile, overlay, checkout, and sources available
until removal is complete.

| Selector | What removal does | Important limit |
|---|---|---|
| `symlinks` | Copies configured source content into home, replacing links | Also materializes missing targets; preserves existing non-link targets |
| `git-hooks` | Removes installed hooks still matching the repository's managed state | Modified/replaced hooks and unrelated hook names are preserved |
| `launcher` | Removes the launcher when it still matches the managed content/state | Leaves modified launchers, the checkout, binary, and PATH entry |
| `script-…` | Runs active overlay scripts' `--remove` | Depends entirely on each script's removal contract |

This is not restoration from backups. Packages, services, registry, global Git
settings, harness settings, shell selection, permissions, completions, WSL,
editor extensions, and APM deployment are not reversed by static uninstall
tasks. Nested symlinks in materialized directory trees are recreated as links,
not flattened copies of everything they reference.

## Validation tasks

`check` uses a separate task set, not the install graph. All commands load
configuration before task filtering, so `--only` is not a workaround for a
malformed required file.

| Selector | Console label | Scope |
|---|---|---|
| `config-warnings` | Validate config warnings | Fails on aggregated configuration diagnostics, including warning-severity findings |
| `symlink-sources` | Validate symlink sources | Checks symlink/glob and chmod sources, including inactive category source definitions |
| `config-files` | Validate config files | Required main TOML inventory; warns when `hooks/` is absent |
| `apm-plugins` | Validate APM plugins | Native `apm pack --dry-run --verbose` for local plugins under the main checkout |
| `shellcheck` | Shellcheck | Shell scripts discovered in the main repository |
| `psscriptanalyzer` | PSScriptAnalyzer | PowerShell scripts discovered in the main repository |

Required main files are `agent-settings.toml`, `chmod.toml`, `git-config.toml`,
`packages.toml`, `registry.toml`, `symlinks.toml`, `system-files.toml`,
`systemd-units.toml`, and `vscode-extensions.toml`, all under `conf/`.

Missing `apm`, `shellcheck`, or `pwsh` produces a visible skip; strict policy
(`--fail-on-skip` or CI) fails on these. If `pwsh` exists but its
PSScriptAnalyzer module does not, the analyzer fails. Local package-shape
checks are not complete validation of remote APM dependencies or every private
overlay script.

Use [Testing](TESTING.md#cli-validation) for canonical commands and the
separate integration/configuration-drift coverage.

## Filtering examples

All examples below preview rather than apply; run from the intended checkout.

```bash
# Just selected home links; prerequisites are assumed satisfied.
dotfiles install --root . --profile base --no-repo-update --only symlinks --dry-run

# Inspect every prerequisite that would be added.
dotfiles tasks --root . --profile desktop --graph update --only apm --with-deps

# Preview configured regular packages and APM updates, without AUR tasks.
dotfiles update --root . --profile desktop --no-repo-update --only "packages,apm" --dry-run
```

For an overlay script, pass the real overlay root, discover its generated
selector with `tasks`, then use that exact `script-…` value with `--only`.
Remember that previewing a script executes its check/preview code.
