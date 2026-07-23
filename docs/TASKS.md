# Task Reference

This reference covers every task currently scheduled by the CLI:

- 24 static tasks in the install/update catalog
- 3 uninstall tasks
- 7 validation tasks
- 1 dynamic task per configured overlay script

Task display names are important because `install --only`, `install --skip`,
`update --only`, and `update --skip` derive case-insensitive selectors from
them. A selector matches the normalized full name, the name with leading action
words removed, or the first word of that canonical name. Matching is not based
on arbitrary substrings.

## Scheduling model

The engine validates the active tasks as a dependency graph. A task becomes
ready after all active dependencies complete successfully. Independent ready
tasks may run in parallel; use `--no-parallel` for deterministic sequential
output without changing dependency order.

Every ordering requirement is an explicit edge. Catalog insertion order is not
scheduling policy. Tasks marked `update_only` are excluded from `install` and
included by `update`; this metadata controls command membership, not ordering.

Built-in mutating tasks are designed to be idempotent and dry-run safe. A task
may report that it is already correct, skipped, or not applicable rather than
performing work. Overlay scripts are opaque external programs, so their
idempotency and dry-run safety depend on the script honoring its contract.

## Install and update tasks

### Catalog overview

| # | Task name | Depends on | Applies to | Purpose |
|---:|---|---|---|---|
| 1 | Install wrapper | - | All platforms | Installs the platform wrapper in `~\.local\bin` |
| 2 | Configure PATH | Install wrapper | All platforms | Ensures `~\.local\bin` is available on user PATH |
| 3 | Enable developer mode | - | Windows | Enables Windows Developer Mode so unprivileged symlink creation is available |
| 4 | Materialize excluded symlinks | - | Profile changes | Preserves managed files before sparse checkout removes their sources |
| 5 | Configure sparse checkout | Materialize excluded symlinks | Git checkouts | Writes profile-derived sparse-checkout rules |
| 6 | Update repository | Configure sparse checkout | Updatable Git checkouts | Pulls repository changes and signals configuration reload |
| 7 | Reload configuration | Update repository | Repository changed | Reloads main and overlay configuration into shared handles |
| 8 | Install Git hooks | Update repository | Git checkouts with `hooks\` | Installs repository-maintained hooks |
| 9 | Install shell completions | Update repository | Linux | Installs generated Zsh completions for `dotfiles` |
| 10 | Report overlay scripts | Reload configuration | Overlay with scripts | Reports scripts discovered after configuration reload |
| 11 | Install packages | - | Arch Linux, Windows | Installs non-AUR packages through pacman or winget |
| 12 | Install paru | Install packages | Arch Linux | Bootstraps the `paru` AUR helper when required |
| 13 | Install AUR packages | Install paru | Arch Linux | Installs package entries marked `aur = true` |
| 14 | Install VS Code extensions | Install packages, Install AUR packages | When VS Code is available | Installs missing declared extensions |
| 15 | Install symlinks | Enable developer mode | All platforms | Converges configured home-directory symlinks |
| 16 | Configure file permissions | Install symlinks | Linux | Applies declared Unix modes to files and trees |
| 17 | Configure default shell | Install packages | Linux | Sets the configured user shell when needed |
| 18 | Configure systemd units | Install packages, Install AUR packages, Install symlinks | Linux with systemd | Enables and starts configured user units |
| 19 | Configure Git | - | Configured platforms | Applies declared global Git settings |
| 20 | Configure registry settings | - | Windows | Converges declared current-user registry values |
| 21 | Configure WSL | - | WSL | Enables systemd and disables Windows PATH injection |
| 22 | Configure Copilot | - | When configured | Converges selected keys in Copilot CLI `settings.json` |
| 23 | Install APM packages | Install packages, Install AUR packages, Install symlinks | When APM inputs exist | Converges merged APM manifests and installed AI tooling |
| 24 | Update APM packages | Install APM packages | `update` only | Advances eligible pinned APM dependencies |

### Host capability and wrapper tasks

#### Enable developer mode

This Windows-only task checks the Developer Mode capability and enables it when
missing. It runs before symlink provisioning because Windows symlink creation
normally requires either Developer Mode or elevation. The task uses lenient
resource processing: unsupported environments are surfaced without hiding real
mutation failures.

#### Install wrapper

Copies the appropriate bootstrap wrapper into `~\.local\bin` as `dotfiles`.
The wrapper remains thin: it locates, downloads, verifies, or builds the Rust
binary and forwards all CLI arguments. Re-running the task replaces stale
wrapper content but does not create a second behavioral implementation.

#### Configure PATH

Runs after **Install wrapper** and ensures `~\.local\bin` can be resolved by the
user. Platform-specific capability methods perform the actual PATH convergence.

### Repository and source tasks

#### Materialize excluded symlinks

When a profile change will exclude paths from sparse checkout, a home-directory
symlink may point at a source Git is about to remove. This task copies the
symlink's content into a real file or directory first. It is a preservation
step, not the uninstall task with a similar purpose.

#### Configure sparse checkout

Runs after preservation. It converts excluded manifest categories into Git
sparse-checkout patterns and applies them to the checkout. It only runs when Git
and an appropriate repository are available.

`conf\manifest.toml` is deliberately not merged from overlays; sparse checkout
describes the main repository's tracked `symlinks\` tree.

#### Update repository

Runs after sparse checkout and updates the current repository when supported.
Successful content changes set an update signal consumed by **Reload
configuration**. Install and update both synchronize the repository; only tasks
explicitly marked update-only are exclusive to the update command.

#### Install Git hooks

Runs after **Update repository**, ensuring the latest hook sources are used.
Hook files are installed from `hooks\` into the checkout's Git hook directory.
The task is not applicable outside a Git checkout or when hook sources are
absent.

#### Install shell completions

Runs after **Update repository** on Linux. The application generates Zsh
completion content from the current Clap command definition and the task
installs it into the managed completion location.

#### Reload configuration

Runs after **Update repository** only when the repository update signal
indicates that content changed. It reloads configuration and updates shared
configuration handles in place. Because overlay configuration changes the task
set, the command executes this task's dependency closure first, rebuilds
dynamic tasks, then runs the remaining static and dynamic tasks together.

#### Report overlay scripts

Runs after **Reload configuration** when an overlay was supplied and
`conf\scripts.toml` produced at least one active script. It only reports the
discovered count. Actual execution is handled by dynamically injected tasks.

### System convergence tasks

#### Configure Git

Reads `conf\git-config.toml` and converges each selected setting using global Git
configuration. Empty configuration produces no work.

#### Configure Copilot

Reads `conf\copilot.toml` and updates only the declared dot-separated keys in
`~\.copilot\settings.json`. Undeclared and volatile CLI-owned keys are
preserved.

#### Install packages

Reads `conf\packages.toml`, separates regular packages from AUR entries, and
uses the active platform provider:

- pacman on Arch Linux
- winget on Windows

The task discovers installed state before applying changes and only requests
elevation when the planned provider action needs it.

#### Install paru

Arch-only bootstrap for the `paru` AUR helper. It is only useful when AUR
packages are selected and the helper is unavailable.

#### Install AUR packages

Installs package entries marked `{ aur = true }` in `conf\packages.toml`. It
uses the AUR helper after its bootstrap prerequisite has completed.

#### Install symlinks

Reads `conf\symlinks.toml`, expands supported source globs, computes
home-relative targets, and creates or corrects links. Main and overlay entries
retain their source repository provenance. On Windows, Developer Mode capability
is established earlier in the graph.

#### Configure file permissions

Linux-only task driven by `conf\chmod.toml`. Directory entries preserve
traversal bits while ordinary files in a recursively processed tree have
execute bits cleared unless explicitly targeted by another entry.

#### Configure default shell

Linux-only task that converges the user's default shell. It runs after package
installation so the desired shell executable can be present.

#### Configure systemd units

Reads `conf\systemd-units.toml` and enables/starts selected user units. It runs
after package, AUR, and symlink tasks because a unit may depend on installed
binaries and linked unit definitions.

#### Configure registry settings

Windows-only task driven by `conf\registry.toml`. It creates or updates
current-user registry values while preserving undeclared values.

#### Install VS Code extensions

Reads `conf\vscode-extensions.toml` and installs missing extensions using an
available VS Code CLI. It runs after regular and AUR package installation so a
newly installed editor can be used in the same run.

#### Install APM packages

Builds the active APM desired state from repository-managed fragments under
`symlinks\apm\config\`, including overlay contributions, then converges the
generated manifest, lock state, plugins, and skills. It runs after package,
AUR, and symlink tasks so the APM executable and inputs are available.

See [APM](APM.md) for manifest ownership and update safeguards.

#### Configure WSL

Runs only inside WSL and converges the required keys in `/etc/wsl.conf` while
preserving unrelated sections and settings. Applying the file may require
elevation, and some WSL settings take effect only after the distribution is
restarted.

### Update-only task

#### Update APM packages

This task is marked update-only and depends on **Install APM packages**. It runs
only for `dotfiles update` and only after APM install state matches the active
merged-manifest fingerprint. That guard prevents a failed or partially
converged install from advancing the lockfile. It invokes APM's idempotent
update directly and compares the lockfile before and after to determine whether
dependency refs advanced.

## Dynamic overlay tasks

After the **Reload configuration** dependency closure completes, the command
rereads the active overlay script configuration and creates one task per
script. If that boundary is absent after filtering, tasks are created from
current configuration before a single graph is run. Each task:

- uses the configured script `name` as its task display name
- has a deterministic identity based on name and path
- participates in `--only` and `--skip` filtering
- uses the script's check mode to determine whether work is required
- uses its dry-run mode during `--dry-run`
- captures and forwards non-empty output through the engine logger

The engine passes `--check` and `--dryrun` as appropriate but cannot prevent a
script from mutating state if the script violates that contract. Although the
underlying resource supports `--remove`, dynamic script tasks are not registered
in the current uninstall catalog.

Scripts are never loaded from the public repository's `conf\` directory. See
[Overlay scripts](CONFIGURATION.md#overlay-scripts).

## Uninstall tasks

Uninstall has a separate, intentionally small catalog.

| Task name | Purpose |
|---|---|
| Materialize symlinks | Replaces every managed home symlink with copied content |
| Remove Git hooks | Removes hooks installed from this repository |
| Remove wrapper | Removes the installed `~\.local\bin\dotfiles` wrapper |

**Materialize symlinks** preserves user-visible files; it does not delete them.
The uninstall command does not attempt to reverse package-manager, systemd,
registry, shell, WSL, APM, editor, or overlay-script changes.

## Validation tasks

`dotfiles test` executes these seven validation tasks through the dependency
scheduler:

| # | Task name | What it checks |
|---:|---|---|
| 1 | Validate config warnings | Emits non-fatal diagnostics collected while loading configuration |
| 2 | Validate symlink sources | Confirms configured symlink sources exist and globs resolve |
| 3 | Validate config files | Requires and parses core TOML files; warns when `hooks\` is absent |
| 4 | Validate manifest sync | Checks exact category-section synchronization between symlinks and sparse-checkout manifest |
| 5 | Validate APM plugins | Validates active APM plugin and package references when APM is available |
| 6 | Shellcheck | Runs ShellCheck on repository shell scripts when installed |
| 7 | PSScriptAnalyzer | Runs PowerShell Script Analyzer whenever `pwsh` is available |

The required core files are:

- `conf\profiles.toml`
- `conf\symlinks.toml`
- `conf\packages.toml`
- `conf\manifest.toml`

ShellCheck and APM validation are not applicable when their executables are
missing. PSScriptAnalyzer is different: the task is selected when `pwsh` is
available, so a missing analyzer module causes that validation task to fail.
Syntax and consistency failures in required configuration also fail the
command. The separate `config_drift` integration test verifies source-path
coverage, compatible subset sections, and the existence of manifest paths.

## Filtering examples

```bash
# Preview tasks selected by the canonical "symlinks" name
dotfiles install --only symlinks --dry-run

# Run package and APM-related update tasks, except AUR tasks
dotfiles update --only "packages,APM" --skip AUR

# Run a dynamic overlay task by its canonical first-word selector
dotfiles install --overlay C:\private-dotfiles --only private
```
