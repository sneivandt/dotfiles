# CLI Task Reference

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

The engine executes strict phase barriers:

1. **Bootstrap** prepares capabilities and the CLI entry point.
2. **Sync** reconciles repository-owned inputs and reloads configuration.
3. **Provision** converges machine state.
4. **Validation** is available for validation workflows.
5. **Update** advances pinned dependencies and runs only for `update`.

Independent tasks within a phase may run in parallel. Explicit dependencies
still apply within a phase. Use `--no-parallel` for deterministic sequential
output; it does not change phase or dependency order.

Built-in mutating tasks are designed to be idempotent and dry-run safe. A task
may report that it is already correct, skipped, or not applicable rather than
performing work. Overlay scripts are opaque external programs, so their
idempotency and dry-run safety depend on the script honoring its contract.

## Install and update tasks

### Catalog overview

| # | Task name | Phase | Applies to | Purpose |
|---:|---|---|---|---|
| 1 | Enable developer mode | Bootstrap | Windows | Enables Windows Developer Mode so unprivileged symlink creation is available |
| 2 | Materialize excluded symlinks | Sync | Profile changes | Preserves managed files before sparse checkout removes their sources |
| 3 | Configure sparse checkout | Sync | Git checkouts | Writes profile-derived sparse-checkout rules |
| 4 | Update repository | Sync | Updatable Git checkouts | Pulls repository changes and signals configuration reload |
| 5 | Configure Git | Provision | Configured platforms | Applies declared global Git settings |
| 6 | Configure Copilot | Provision | When configured | Converges selected keys in Copilot CLI `settings.json` |
| 7 | Install Git hooks | Sync | Git checkouts with `hooks\` | Installs repository-maintained hooks |
| 8 | Install shell completions | Sync | Linux | Installs generated Zsh completions for `dotfiles` |
| 9 | Install packages | Provision | Arch Linux, Windows | Installs non-AUR packages through pacman or winget |
| 10 | Install paru | Provision | Arch Linux | Bootstraps the `paru` AUR helper when required |
| 11 | Install AUR packages | Provision | Arch Linux | Installs package entries marked `aur = true` |
| 12 | Install symlinks | Provision | All platforms | Converges configured home-directory symlinks |
| 13 | Configure file permissions | Provision | Linux | Applies declared Unix modes to files and trees |
| 14 | Configure default shell | Provision | Linux | Sets the configured user shell when needed |
| 15 | Configure systemd units | Provision | Linux with systemd | Enables and starts configured user units |
| 16 | Configure registry settings | Provision | Windows | Converges declared current-user registry values |
| 17 | Install VS Code extensions | Provision | When VS Code is available | Installs missing declared extensions |
| 18 | Install APM packages | Provision | When APM inputs exist | Converges merged APM manifests and installed AI tooling |
| 19 | Update APM packages | Update | `update` only | Advances eligible pinned APM dependencies |
| 20 | Install wsl.conf | Provision | WSL | Installs the repository's WSL system configuration |
| 21 | Report overlay scripts | Sync | Overlay with scripts | Reports scripts discovered after configuration reload |
| 22 | Install wrapper | Bootstrap | All platforms | Installs the platform wrapper in `~\.local\bin` |
| 23 | Configure PATH | Bootstrap | All platforms | Ensures `~\.local\bin` is available on user PATH |
| 24 | Reload configuration | Sync | Repository changed | Reloads main and overlay configuration into shared handles |

### Bootstrap tasks

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

### Synchronization tasks

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
configuration**. Install and update both synchronize the repository; the Update
phase is about dependency versions, not whether Git is pulled.

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
configuration handles in place, so later tasks use refreshed data without
rebuilding the task graph.

#### Report overlay scripts

Runs after **Reload configuration** when an overlay was supplied and
`conf\scripts.toml` produced at least one active script. It only reports the
discovered count. Actual execution is handled by dynamically injected
Provision-phase tasks.

### Provisioning tasks

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

#### Install wsl.conf

Runs only inside WSL and installs the repository's `wsl.conf` system
configuration. Applying the file may require elevation, and some WSL settings
take effect only after the distribution is restarted.

### Update task

#### Update APM packages

This is the sole static Update-phase task. It runs only for `dotfiles update`
and only after APM install state matches the active merged-manifest
fingerprint. That guard prevents a failed or partially converged install from
advancing the lockfile. If no outdated dependencies are reported, it skips
without changing versions.

## Dynamic overlay tasks

After the Sync phase completes, the command rereads the active overlay script
configuration and creates one task per script. Each task:

- uses the configured script `name` as its task display name
- has a deterministic identity based on name and path
- runs in the Provision phase
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

| Task name | Phase | Purpose |
|---|---|---|
| Materialize symlinks | Provision | Replaces every managed home symlink with copied content |
| Remove Git hooks | Sync | Removes hooks installed from this repository |
| Remove wrapper | Bootstrap | Removes the installed `~\.local\bin\dotfiles` wrapper |

**Materialize symlinks** preserves user-visible files; it does not delete them.
The uninstall command does not attempt to reverse package-manager, systemd,
registry, shell, WSL, APM, editor, or overlay-script changes.

## Validation tasks

`dotfiles test` executes these seven tasks in order:

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
