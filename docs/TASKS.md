# Task Reference

This reference covers the visible install, update, uninstall, validation, and
dynamic overlay tasks scheduled by the CLI. Run `dotfiles tasks` for the active
machine-readable list of selectors, labels, and command membership.

Task metadata separates four concerns:

- scheduler identity, used for dependencies and duplicate detection
- stable selector, used by `--only` and `--skip`
- display label, used in console rows
- visibility, which keeps internal orchestration out of discovery and totals

Selectors are case-insensitive after punctuation and whitespace are normalized
to hyphens. Matching is exact against either the stable selector or the full
normalized display label; it does not remove action words, use the first word,
or perform substring matching.

## Scheduling model

The engine validates the active tasks as a dependency graph. A task becomes
ready after all active dependencies complete successfully. Independent ready
tasks may run in parallel, and visible rows are printed in natural completion
order. `--no-parallel` runs independent work sequentially but does not define a
documented display order.

Every ordering requirement is an explicit edge. Catalog insertion order is not
scheduling policy. Tasks marked `update_only` are excluded from `install` and
included by `update`; this metadata controls command membership, not ordering.

Built-in mutating tasks are designed to be idempotent and dry-run safe. A task
may report that it is already correct, skipped, or not applicable rather than
performing work. Overlay scripts are opaque external programs, so their
idempotency and dry-run safety depend on the script honoring its contract.

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
| `systemd` | Systemd units | install, update | Enables and starts configured user units |
| `registry` | Windows registry | install, update | Converges declared current-user values |
| `vscode-extensions` | VS Code extensions | install, update | Installs missing declared extensions |
| `apm` | APM packages | install, update | Converges merged APM manifests and AI tooling |
| `apm-update` | APM package updates | update | Advances eligible pinned APM dependencies |
| `wsl-config` | WSL configuration files | install, update | Converges required `/etc/wsl.conf` settings |
| `launcher` | Dotfiles launcher | install, update, uninstall | Installs or removes the platform wrapper |
| `path` | Shell PATH | install, update | Ensures the launcher directory is on user PATH |

`Reload configuration` and `Report overlay scripts` are internal orchestration
tasks. They keep their scheduler identities and run-log entries, but do not
appear in `dotfiles tasks`, normal console rows, or aggregate totals.

### Host capability and wrapper tasks

#### Windows Developer Mode

This Windows-only task checks the Developer Mode capability and enables it when
missing. It runs before symlink provisioning because Windows symlink creation
normally requires either Developer Mode or elevation. The task uses lenient
resource processing: unsupported environments are surfaced without hiding real
mutation failures.

It is the only task that always needs administrator rights, and only on the
first run — once Developer Mode is set, no Windows task requires elevation.
When it cannot elevate, it and every task that depends on it are skipped and
the rest of the run proceeds.

#### Dotfiles launcher

Copies the appropriate bootstrap wrapper into `~/.local/bin` as `dotfiles`.
The wrapper remains thin: it locates, downloads, verifies, or builds the Rust
binary and forwards all CLI arguments. Re-running the task replaces stale
wrapper content but does not create a second behavioral implementation.

#### Shell PATH

Runs after **Dotfiles launcher** and ensures `~/.local/bin` can be resolved by the
user. Platform-specific capability methods perform the actual PATH convergence.

### Repository and source tasks

#### Excluded home files

When a profile change will exclude paths from sparse checkout, a home-directory
symlink may point at a source Git is about to remove. This task copies the
symlink's content into a real file or directory first. It is a preservation
step, not the uninstall task with a similar purpose.

#### Sparse checkout

Runs after preservation. It converts excluded manifest categories into Git
sparse-checkout patterns and applies them to the checkout. It only runs when Git
and an appropriate repository are available.

`conf/manifest.toml` is deliberately not merged from overlays; sparse checkout
describes the main repository's tracked `symlinks/` tree.

#### Dotfiles repository

Runs after sparse checkout and updates the current repository when supported.
Successful content changes set an update signal consumed by **Reload
configuration**. Install and update both synchronize the repository; only tasks
explicitly marked update-only are exclusive to the update command.

#### Git hooks

Runs after **Dotfiles repository**, ensuring the latest hook sources are used.
Hook files are installed from `hooks/` into the checkout's Git hook directory.
The task is not applicable outside a Git checkout or when hook sources are
absent.

#### Shell completions

Runs after **Dotfiles repository** on Linux. The application generates Zsh
completion content from the current Clap command definition and the task
installs it into the managed completion location.

#### Reload configuration

Runs after **Dotfiles repository** only when the repository update signal
indicates that content changed. It reloads configuration and updates shared
configuration handles in place. Because overlay configuration changes the task
set, the command executes this task's dependency closure first, rebuilds
dynamic tasks, then runs the remaining static and dynamic tasks together.

#### Report overlay scripts

Runs after **Reload configuration** when an overlay was supplied and
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

On Windows the task never elevates itself. winget installs are attempted with
`--scope user` first and retried unscoped only when no user-scope installer
exists. A package whose installer still demands administrator rights is
reported as skipped rather than failing the task, so the rest of the run
continues. Re-run `dotfiles install --only packages` from an elevated terminal
to finish those.

#### Paru package manager

Arch-only bootstrap for the `paru` AUR helper. It queries `pacman -Q paru` and
runs `/usr/bin/paru --version` in the current system context; under
`install-arch`, both commands run inside the target chroot. A missing helper is
installed and a present but unusable helper is rebuilt from AUR source against
the current system libraries. Before cloning, the task requires `git`,
`makepkg`, and `sudo`, then resolves Cargo and runs `cargo --version` so an
unconfigured `rustup` proxy fails with remediation guidance instead of failing
inside `makepkg`. The target package and executable must pass the same check
before dependent tasks run, which invoke that exact `/usr/bin/paru` rather than
resolving a host or stale PATH entry.

#### AUR packages

Installs package entries marked `{ aur = true }` in `conf/packages.toml`. It
uses the AUR helper after its bootstrap prerequisite has completed.

#### Home symlinks

Reads `conf/symlinks.toml`, expands supported source globs, computes
home-relative targets, and creates or corrects links. Main and overlay entries
retain their source repository provenance. On Windows, Developer Mode capability
is established earlier in the graph.

#### File permissions

Linux-only task driven by `conf/chmod.toml`. Directory entries preserve
traversal bits while ordinary files in a recursively processed tree have
execute bits cleared unless explicitly targeted by another entry.

#### Default shell

Linux-only task that converges the user's default shell. It runs after package
installation so the desired shell executable can be present. State comes from
the account database rather than the invoking process's `SHELL` variable. A
root invocation uses `usermod` directly; an unprivileged non-interactive
invocation uses passwordless or cached sudo when available, while a normal
interactive run retains the `chsh` path.

#### Systemd units

Reads `conf/systemd-units.toml` and enables/starts selected user units. It runs
after package, AUR, and symlink tasks because a unit may depend on installed
binaries and linked unit definitions. When the user manager is unavailable in
a fresh-install chroot, it creates the per-user enablement links declared by
the units' `[Install]` sections and leaves startup to the first real login.

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

This task is marked update-only and depends on **APM packages**. It runs
only for `dotfiles update` and only after APM install state matches the active
merged-manifest fingerprint. That guard prevents a failed or partially
converged install from advancing the lockfile. It invokes APM's idempotent
update directly and compares the lockfile before and after to determine whether
dependency refs advanced. The comparison ignores APM's volatile `generated_at`
stamp so a re-serialized-but-unchanged lockfile is not reported as a change.

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

The engine passes `--check` and `--dryrun` as appropriate but cannot prevent a
script from mutating state if the script violates that contract. Although the
underlying resource supports `--remove`, dynamic script tasks are not registered
in the current uninstall catalog.

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
| `config-warnings` | Validate config warnings | Emits non-fatal diagnostics collected while loading configuration |
| `symlink-sources` | Validate symlink sources | Confirms configured symlink and file-permission sources exist and globs resolve |
| `config-files` | Validate config files | Requires and parses core TOML files; warns when `hooks/` is absent |
| `manifest-sync` | Validate manifest sync | Checks exact category-section synchronization between symlinks and sparse-checkout manifest |
| `apm-plugins` | Validate APM plugins | Validates active APM plugin and package references when APM is available |
| `shellcheck` | Shellcheck | Runs ShellCheck on repository shell scripts when installed |
| `psscriptanalyzer` | PSScriptAnalyzer | Runs PowerShell Script Analyzer whenever `pwsh` is available |

The required core files are:

- `conf/profiles.toml`
- `conf/symlinks.toml`
- `conf/packages.toml`
- `conf/manifest.toml`

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
