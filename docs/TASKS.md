# Task reference

This page describes the CLI's visible install, update, uninstall, validation,
and dynamic overlay tasks. Run `dotfiles tasks` for the active list of
selectors, labels, and command membership.

Each task has separate metadata for:

- scheduler identity, used for dependencies and duplicate detection
- stable selector, used by `--only` and `--skip`
- display label, used in console rows
- visibility, which keeps internal orchestration out of discovery and totals

Selectors are case-insensitive after punctuation and whitespace are normalized
to hyphens. Matching is exact against either the stable selector or the full
normalized display label; it does not remove action words, use the first word,
or perform substring matching.

## Scheduling model

The engine validates active tasks as a dependency graph. A task becomes ready
after its active dependencies succeed. Independent ready tasks may run in
parallel, so visible rows appear in completion order. `--no-parallel` runs
independent work sequentially but does not guarantee a display order.

Every ordering requirement is an explicit edge. Catalog insertion order is not
scheduling policy. Tasks marked `update_only` are excluded from `install` and
included by `update`; this metadata controls command membership, not ordering.

Built-in mutating tasks are idempotent and dry-run safe. A task may report
current, skipped, or not applicable without performing work. Overlay scripts
are external programs, so each script must honor the idempotency and dry-run
contract itself.

## Install and update tasks

### Catalog overview

| Selector | Task label | Commands | Purpose |
|---|---|---|---|
| `developer-mode` | Windows Developer Mode | install, update | Enables unprivileged symlink creation |
| `excluded-symlinks` | Excluded home files | install, update | Preserves managed files before sparse checkout removes their sources |
| `sparse-checkout` | Sparse checkout | install, update | Writes profile-derived sparse-checkout rules |
| `repository` | Dotfiles repository | install, update | Synchronizes repository content |
| `git` | Git settings | install, update | Applies declared global Git settings |
| `agent-settings` | Agent settings | install, update | Converges selected harness settings |
| `git-hooks` | Git hooks | install, update, uninstall | Installs or removes repository-maintained hooks |
| `completions` | Shell completions | install, update | Installs generated shell completions |
| `packages` | System packages | install, update | Installs non-AUR packages through pacman or winget |
| `paru` | Paru package manager | install, update | Bootstraps the `paru` AUR helper |
| `aur-packages` | AUR packages | install, update | Installs package entries marked `aur = true` |
| `symlinks` | Home symlinks | install, update, uninstall | Converges or materializes managed home links |
| `file-permissions` | File permissions | install, update | Applies declared Unix modes |
| `shell` | Default shell | install, update | Converges the configured login shell |
| `systemd` | Systemd units | install, update | Enables and starts configured units, or enables user units offline during target provisioning |
| `registry` | Windows registry | install, update | Converges declared current-user values |
| `vscode-extensions` | VS Code extensions | install, update | Installs missing declared extensions, or schedules them for first login during target provisioning |
| `apm` | APM packages | install, update | Converges merged APM manifests and AI tooling |
| `apm-update` | APM package updates | update | Advances eligible pinned APM dependencies |
| `wsl-config` | WSL configuration files | install, update | Converges required `/etc/wsl.conf` settings |
| `launcher` | Dotfiles launcher | install, update, uninstall | Installs or removes the platform wrapper |
| `path` | Shell PATH | install, update | Ensures the launcher directory is on user PATH |

`Reload configuration`, `Reconcile updated checkout`, and `Report overlay
scripts` are internal orchestration tasks. They keep their scheduler identities
and run-log entries, but do not appear in `dotfiles tasks`, normal console rows,
or aggregate totals.

### Host capability and wrapper tasks

#### Windows Developer Mode

This Windows-only task checks Developer Mode and enables it when needed. It runs
before symlink provisioning because Windows normally requires Developer Mode or
elevation to create symlinks. Unsupported environments are reported without
hiding mutation failures.

Enabling Developer Mode is the only Windows catalog mutation that inherently
needs administrator rights. Pending file symlinks can also require elevation
while Developer Mode is off; directory links can fall back to junctions. After
Developer Mode is enabled, no Windows task requires brokered elevation. If
elevation is unavailable, the CLI skips the affected tasks and their dependents,
then continues the rest of the run.

#### Dotfiles launcher

Copies the platform wrapper into `~/.local/bin` as `dotfiles`. The wrapper
locates, downloads, verifies, or builds the Rust binary, then forwards all CLI
arguments. Rerunning the task replaces stale wrapper content. Command behavior
remains in the Rust CLI.

#### Shell PATH

Runs after **Dotfiles launcher** and ensures `~/.local/bin` can be resolved by the
user. Platform-specific capability methods perform the actual PATH convergence.

### Repository and source tasks

#### Excluded home files

Before a profile change excludes paths from sparse checkout, a home symlink may
still point to a source Git will remove. This task first replaces that link with
a real file or directory. It preserves content during profile changes; it is
not an uninstall task.

#### Sparse checkout

After preservation, this task converts excluded manifest categories into Git
sparse-checkout patterns and applies them. It runs only when Git and a suitable
repository are available.

`conf/manifest.toml` is deliberately not merged from overlays; sparse checkout
describes the main repository's tracked `symlinks/` tree.

#### Dotfiles repository

Runs after sparse checkout and updates the current repository when supported.
Successful content changes set an update signal consumed by **Reload
configuration**. Install and update both synchronize the repository; only tasks
explicitly marked update-only are exclusive to the update command.
With `--offline`, repository synchronization is omitted and the current
checkout is treated as the desired source; the normal preservation and sparse
checkout tasks still converge it without activating the post-update reload
boundary.

#### Git hooks

Runs after **Dotfiles repository**, so it uses the latest hook sources. It
installs files from `hooks/` into the checkout's Git hook directory. The task
does not apply outside a Git checkout or when hook sources are absent.

#### Shell completions

The application generates completions from the current Clap command definition
after **Dotfiles repository** runs. Linux writes Zsh completions beneath the
managed `symlinks/config/zsh/completions` tree. Windows writes PowerShell
completions to `~/.config/powershell/profile.d`.

#### Reload configuration

Runs after **Dotfiles repository** only when the repository update signal
indicates that content changed. It re-resolves the selected profile from the
updated `profiles.toml`, reloads configuration, and updates shared configuration
handles by publishing one immutable configuration generation. Tasks cannot
observe a mix of sections from before and after the reload.

If this boundary or a prerequisite fails, the CLI does not discover late
dynamic tasks from inconsistent state. Independent static tasks continue.
Tasks with failed blocking prerequisites remain skipped.

#### Reconcile updated checkout

Runs after **Reload configuration** and reapplies preservation and
sparse-checkout rules from the refreshed profile and manifest. Profile,
manifest, and overlay changes can affect later task discovery. The command
therefore executes this task's dependency closure first, rebuilds dynamic tasks,
then runs the remaining static and dynamic tasks together.

#### Report overlay scripts

Runs after **Reconcile updated checkout** when an overlay was supplied and
`conf/scripts.toml` produced at least one active script. It only reports the
discovered count. Actual execution is handled by dynamically injected tasks.

### System convergence tasks

#### Git settings

Reads `conf/git-config.toml` and converges each selected setting using global Git
configuration. Empty configuration produces no work.

#### Agent settings

Reads `conf/agent-settings.toml` and updates declared dot-separated keys in
Copilot's JSON settings and Codex's TOML settings. Undeclared and volatile
harness-owned keys are preserved.

#### System packages

Reads `conf/packages.toml`, separates regular packages from AUR entries, and
uses the active platform provider:

- pacman on Arch Linux
- winget on Windows

The task discovers installed state before applying changes and only requests
elevation when the planned provider action needs it.

On Windows, the task never elevates itself. It tries winget with `--scope user`
first, then retries without a scope only when no user-scope installer exists. If
an installer still requires administrator rights, the package is marked skipped
and the rest of the run continues. Run
`dotfiles install --only packages` from an elevated terminal to finish those
packages.

#### Paru package manager

This Arch-only task bootstraps the `paru` AUR helper. It queries
`pacman -Q paru` and runs `/usr/bin/paru --version` in the current system
context. Under `install-arch`, both commands run inside the target chroot. The
task installs a missing helper. It rebuilds an installed but unusable helper
from AUR source against the current system libraries.

Before cloning, the task requires `git`, `makepkg`, and `sudo`. It then resolves
Cargo and runs `cargo --version`. An unconfigured `rustup` proxy therefore fails
with remediation guidance before `makepkg` starts. Both the target package and
executable must pass the same check before dependent tasks run. Those tasks call
the exact `/usr/bin/paru` path instead of resolving a host or stale PATH entry.

#### AUR packages

Installs package entries marked `{ aur = true }` in `conf/packages.toml`. It
uses the AUR helper after its bootstrap prerequisite has completed.

#### Home symlinks

Reads `conf/symlinks.toml`, expands supported source globs, computes
home-relative targets, and creates or corrects links. Main and overlay entries
keep their source repository. On Windows, the graph establishes Developer Mode
first.

#### File permissions

Linux-only task driven by `conf/chmod.toml`. Directory entries preserve
traversal bits while ordinary files in a recursively processed tree have
execute bits cleared unless explicitly targeted by another entry.

#### Default shell

This Linux-only task sets the user's default shell after package installation,
when the desired executable is available. It reads state from the account
database instead of the invoking process's `SHELL` variable. A root invocation
uses `usermod` directly. An unprivileged non-interactive invocation uses
passwordless or cached sudo when available. A normal interactive run uses
`chsh`.

#### Systemd units

Reads `conf/systemd-units.toml` and enables/starts selected units. Bare strings
use user scope; table entries can select `user` or `system` scope. System units
use `sudo` when they need enablement. The task runs after package, AUR, and
symlink tasks because a unit may depend on installed binaries and linked unit
definitions. When the user manager is unavailable in a fresh-install chroot, it
creates per-user enablement links for user units and leaves startup to the first
real login.

#### Windows registry

Windows-only task driven by `conf/registry.toml`. It creates or updates
current-user registry values while preserving undeclared values.

#### VS Code extensions

Reads `conf/vscode-extensions.toml` and installs missing extensions using an
available VS Code CLI. It runs after regular and AUR package installation so a
newly installed editor can be used in the same run.

#### APM packages

Builds the active APM desired state from repository-managed fragments under
`symlinks/apm/config/`, including overlay contributions, then converges the
generated manifest, lock state, plugins, and skills. It runs after package,
AUR, and symlink tasks so the APM executable and inputs are available.

See [APM](APM.md) for manifest ownership and update safeguards.

#### WSL configuration files

Runs only inside WSL and converges the required keys in `/etc/wsl.conf` while
preserving unrelated sections and settings. Applying the file may require
elevation, and some WSL settings take effect only after the distribution is
restarted.

### Update-only task

#### APM package updates

This update-only task depends on **APM packages**. It runs only during
`dotfiles update` and only when APM install state matches the active
merged-manifest fingerprint. The guard prevents failed or partial install state
from advancing the lockfile. The task invokes APM's idempotent update and
compares the lockfile before and after. It ignores APM's volatile `generated_at`
stamp, so reserializing an unchanged lockfile does not count as a change.

## Dynamic overlay tasks

After the internal **Reload configuration** dependency closure completes, the
command rereads the active overlay script configuration and creates one task
per script. If that boundary is absent after filtering, tasks are created from
current configuration before a single graph is run. Each task:

- uses the configured script `name` as its task display name
- uses `script-<normalized-name>` as its stable selector
- has a deterministic identity based on name and path
- participates in `--only` and `--skip` filtering
- appears in `dotfiles tasks` when active
- uses the script's check mode to determine whether work is required
- uses its dry-run mode during `--dry-run`
- captures and forwards non-empty output through the engine logger

The engine passes `--check` and `--dryrun` as needed, but it cannot stop a
script that ignores the contract from changing state. The underlying resource
supports `--remove`; dynamic script tasks are not registered in the current
uninstall catalog.

Scripts are never loaded from the public repository's `conf/` directory. See
[Overlay scripts](CONFIGURATION.md#overlay-scripts).

## Uninstall tasks

Uninstall reuses the stable selectors and labels shown in discovery:

| Selector | Task label | Purpose |
|---|---|---|
| `symlinks` | Home symlinks | Replaces every managed home symlink with copied content |
| `git-hooks` | Git hooks | Removes hooks installed from this repository |
| `launcher` | Dotfiles launcher | Removes the installed `~/.local/bin/dotfiles` wrapper |

**Home symlinks** preserves user-visible files; it does not delete them.
The uninstall command does not attempt to reverse package-manager, systemd,
registry, shell, WSL, APM, editor, or overlay-script changes.

## Validation tasks

`dotfiles test` executes these seven validation tasks through the dependency
scheduler:

| Selector | Task label | What it checks |
|---|---|---|
| `config-warnings` | Validate config warnings | Reports loader diagnostics, including APM fragment, local plugin, dependency-field, and MCP validation |
| `symlink-sources` | Validate symlink sources | Confirms configured symlink and file-permission sources exist and globs resolve |
| `config-files` | Validate config files | Confirms all required main TOML files exist; warns when `hooks/` is absent |
| `manifest-sync` | Validate manifest sync | Checks exact category-section synchronization between symlinks and sparse-checkout manifest |
| `apm-plugins` | Validate APM plugins | Runs `apm pack --dry-run --verbose` for each local plugin when APM is available |
| `shellcheck` | Shellcheck | Runs ShellCheck on repository shell scripts when installed |
| `psscriptanalyzer` | PSScriptAnalyzer | Runs PowerShell Script Analyzer whenever `pwsh` is available |

The required main files are:

- `conf/agent-settings.toml`
- `conf/chmod.toml`
- `conf/git-config.toml`
- `conf/manifest.toml`
- `conf/packages.toml`
- `conf/profiles.toml`
- `conf/registry.toml`
- `conf/symlinks.toml`
- `conf/systemd-units.toml`
- `conf/vscode-extensions.toml`

ShellCheck and APM validation are not applicable when their executables are
missing. PSScriptAnalyzer is different: the task is selected when `pwsh` is
available, so a missing analyzer module causes that validation task to fail.
Syntax and consistency failures in required configuration also fail the
command. The separate `config_drift` integration test verifies source-path
coverage, compatible subset sections, and the existence of manifest paths.

## Filtering examples

```bash
# Preview the stable "symlinks" selector
dotfiles install --only symlinks --dry-run

# Run package and APM-related update tasks, except AUR tasks
dotfiles update --only "packages,apm,apm-update" --skip aur-packages

# Run a dynamic overlay task by its generated stable selector
dotfiles install --overlay C:\private-dotfiles --only script-private-tools
```
