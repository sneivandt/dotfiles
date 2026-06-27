# Usage Guide

Comprehensive guide to using the dotfiles installation and management system.

## Installation

### Linux

**Basic installation with profile selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
```

**Interactive profile selection (first time):**
```bash
./dotfiles.sh install
# You'll be prompted to select from available profiles
# Your selection is saved for future runs
```

**Subsequent runs (uses saved profile):**
```bash
./dotfiles.sh install
# Automatically uses the profile you selected previously
```

### Windows

**Initial installation:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

## Command Line Options

### Linux (`dotfiles.sh`)

**Synopsis:**
```
dotfiles.sh [--build] install   [-p PROFILE] [-d] [-v]
dotfiles.sh [--build] update    [-p PROFILE] [-d] [-v]
dotfiles.sh [--build] uninstall [-p PROFILE] [-d] [-v]
dotfiles.sh [--build] test      [-p PROFILE] [-v]
dotfiles.sh [--build] logs      [-v]
dotfiles.sh [--build] version
```

**Options:**

- **`install`** - Converge the system to the declared state (does not advance pinned versions)
- **`update`** - Everything `install` does, **plus** advancing pinned dependency versions (currently APM plugin dependencies via `apm update`)
- **`uninstall`** - Remove installed dotfiles (managed symlinks)
- **`test`** - Run configuration validation
- **`logs`** - Print the most recent dotfiles operation log
- **`version`** - Print version information
- **`--build`** - Build and run from source (requires `cargo`)
- **`-p, --profile PROFILE`** - Use specific profile (base, desktop)
- **`--overlay DIR`** - Use a private overlay repository for additional configuration
- **`-v, --verbose`** - Enable verbose logging
- **`-d, --dry-run`** - Preview changes without modifying system

`install` and `update` run the same base task graph and accept the same
`--skip` / `--only` selectors. `update` additionally schedules the final
Update phase to advance pinned dependency versions, while `install` leaves them
pinned.

### Windows (`dotfiles.ps1`)

**Synopsis:**
```powershell
.\dotfiles.ps1 [--build] install -p desktop [-d] [-v]
.\dotfiles.ps1 [--build] update -p desktop [-d] [-v]
.\dotfiles.ps1 [--build] uninstall [-d]
.\dotfiles.ps1 [--build] test
.\dotfiles.ps1 [--build] logs [-v]
.\dotfiles.ps1 [--build] version
```

**Parameters:**

- **`--build`** - Build and run from source (requires `cargo`)
- **`-p PROFILE`** - Use specific profile (base, desktop)
- **`--overlay DIR`** - Use a private overlay repository for additional configuration
- **`-d`** - Preview changes without applying (dry-run)
- **`-v, --verbose`** - Enable verbose logging

## Common Workflows

### First-Time Setup

**Linux with interactive selection:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install
# Select profile when prompted
# Installation proceeds with your selection
```

**Linux with explicit profile:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
```

**Windows:**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

### Updating Dotfiles

`install` is the normal convergence command. It may update the dotfiles binary
first, then attempts a safe fast-forward-only repository sync, reloads
configuration if the repo changed, and applies the declared machine state. It
does **not** advance pinned dependency versions.

`update` does everything `install` does **and** adds a final **Updating
dependencies** phase that advances pinned dependency versions (currently APM
plugin dependencies, via `apm update`). Because this is a separate phase
that runs after everything else, `update` output ends with a `:: Updating
dependencies` section; `install` has no such phase. Use `update` when you want
to pull in newer dependency versions; use `install` for a reproducible,
version-stable apply.

**Linux (uses saved profile):**
```bash
cd ~/dotfiles
./dotfiles.sh update      # install + advance pinned dependency versions
```

To re-apply configuration without bumping any pinned versions, run
`./dotfiles.sh install` instead.

**Windows:**
```powershell
cd ~\dotfiles
.\dotfiles.ps1 update -p desktop
```

### Previewing Changes

**Before running installation:**
```bash
./dotfiles.sh install -p desktop -d
# Shows what would be done without making changes
```

**Windows:**
```powershell
.\dotfiles.ps1 install -p desktop -d
```

### Switching Profiles

**Change profile and apply changes:**
```bash
./dotfiles.sh install -p base
# Switches to base profile
# Removes desktop-specific symlinks and files
# New profile is saved for future runs
```

### Uninstalling

`uninstall` is intentionally conservative. It removes the pieces that this
project can safely detach without guessing the user's desired system state:
managed symlinks, installed Git hooks, and the wrapper entry point. It does not
try to roll back packages, registry values, systemd enablement, shell changes,
VS Code extensions, AI tooling, PAM/WSL configuration, or
overlay script effects.

When removing a managed symlink, the current source content is materialized into
the target path first, so uninstalling does not leave the user without the file
or directory that had been linked.

**Remove managed symlinks, Git hooks, and wrapper:**
```bash
./dotfiles.sh uninstall
# Does not remove packages or other system/editor configuration
```

**Preview uninstallation:**
```bash
./dotfiles.sh uninstall -d
```

### Testing

**Run configuration validation:**
```bash
./dotfiles.sh test
# Validates TOML file syntax
# Checks profile definitions
# Verifies configuration references
```

### Checking Version

```bash
./dotfiles.sh version
# Prints the installed binary version
```

## What Gets Installed

The installation process handles different components based on your profile:

### Linux Task Groups

Before the scheduler starts, **Self-Update** may update the dotfiles binary from
the latest GitHub release and re-exec the process so the rest of the run uses
the new engine.

The scheduled work then runs in phase order: Bootstrap, Sync, Provision, and
Update. Rows within the same phase are inventory, not a strict sequence; tasks
run in parallel whenever their dependencies allow.

| Phase | Task | Description |
| --- | --- | --- |
| Bootstrap | Install wrapper | Installs `dotfiles` wrapper to `~/.local/bin/`. |
| Bootstrap | Configure PATH | Ensures `~/.local/bin` is on PATH after the wrapper task completes. |
| Sync | Configure sparse checkout | Excludes files based on profile. |
| Sync | Update repository | Pulls latest changes (`git pull --ff-only`) after sparse checkout is configured. |
| Sync | Install Git hooks | Copies repository git hooks into `.git/hooks/` after repository update. |
| Sync | Reload configuration | Reloads config after repository update. |
| Sync | Install shell completions | Writes the zsh completion script into the managed `symlinks/config/zsh/completions/` directory after repository update. |
| Sync | Load overlay scripts | Discovers overlay script tasks after config reload, when `--overlay` is set. |
| Provision | Install packages | Installs packages from `conf/packages.toml` using pacman. |
| Provision | Install paru | Bootstraps paru AUR helper (Arch Linux only). |
| Provision | Install AUR packages | Installs AUR packages via paru after paru is available (Arch Linux only). |
| Provision | Install symlinks | Links files from `symlinks/` to `$HOME`. |
| Provision | Configure file permissions | Applies file permissions from `conf/chmod.toml` after symlinks exist. |
| Provision | Configure Git | Applies git configuration. |
| Provision | Configure Copilot | Applies Copilot CLI settings from `conf/copilot.toml`. |
| Provision | Configure default shell | Sets the default shell after packages are installed. |
| Provision | Configure systemd units | Enables and starts user or system units from `conf/systemd-units.toml` after symlinks exist. |
| Provision | Install VS Code extensions | Installs extensions from `conf/vscode-extensions.toml`. |
| Provision | Install APM packages | Merges every `~/.apm/config/*.yml` fragment into `~/.apm/apm.yml` and runs `apm install` when the manifest or lockfile changed. This converges to the locked manifest and never advances locked refs ([Microsoft APM](https://github.com/microsoft/apm)). |
| Provision | Configure PAM services | Installs custom PAM service files (Arch Linux + desktop profile only, uses sudo). |
| Provision | Write wsl.conf | Writes `/etc/wsl.conf` with `generateResolvConf = true` under `[network]` (WSL only, via sudo when not root). |
| Provision | Overlay scripts | Runs custom scripts loaded from the overlay repository (when `--overlay` is set). |
| Update | Update APM packages | Runs `apm outdated -g` and, when locked dependencies are stale, `apm update -g --yes` to advance them to the latest matching refs. This phase only runs under `dotfiles update` and is absent from `install`. |

### Windows Task Groups

Before the scheduler starts, **Self-Update** may update the dotfiles binary from
the latest GitHub release and re-exec the process.

| Phase | Task | Description |
| --- | --- | --- |
| Bootstrap | Enable developer mode | Enables Windows developer mode (required for symlinks). |
| Bootstrap | Install wrapper | Installs the platform `dotfiles` wrapper script so the CLI is on PATH from any directory. |
| Bootstrap | Configure PATH | Ensures dotfiles bin directory is on PATH after the wrapper task completes. |
| Sync | Configure sparse checkout | Excludes files based on profile. |
| Sync | Update repository | Pulls latest changes (`git pull --ff-only`) after sparse checkout is configured. |
| Sync | Install Git hooks | Copies repository git hooks into `.git/hooks/` after repository update. |
| Sync | Reload configuration | Reloads config after repository update. |
| Sync | Load overlay scripts | Discovers overlay script tasks after config reload, when `--overlay` is set. |
| Provision | Install packages | Installs packages using winget. |
| Provision | Install symlinks | Links files from `symlinks/` to `%USERPROFILE%`. |
| Provision | Configure Git | Sets `core.symlinks=true`, `core.autocrlf=false`, and credential helper. |
| Provision | Configure Copilot | Applies Copilot CLI settings from `conf/copilot.toml`. |
| Provision | Configure registry settings | Configures registry from `conf/registry.toml`. |
| Provision | Install VS Code extensions | Installs extensions from `conf/vscode-extensions.toml`. |
| Provision | Install APM packages | Merges every `~/.apm/config/*.yml` fragment into `~/.apm/apm.yml` and runs `apm install` when the manifest or lockfile changed. This converges to the locked manifest and never advances locked refs ([Microsoft APM](https://github.com/microsoft/apm)). |
| Provision | Overlay scripts | Runs custom scripts loaded from the overlay repository (when `--overlay` is set). |
| Update | Update APM packages | Runs `apm outdated -g` and, when locked dependencies are stale, `apm update -g --yes` to advance them to the latest matching refs. This phase only runs under `dotfiles update` and is absent from `install`. |

## Verbose Mode

Enable verbose logging to see detailed operation information:

```bash
./dotfiles.sh install -v
```

**Verbose output includes:**
- Stage headers for each task (`==>` markers)
- Per-item detail (symlinks, packages, etc.)
- Operations being skipped (with reasons)
- Full per-task summary grouped by domain

**Default (non-verbose) output** shows compact inline task-result lines as each
task's buffered output is flushed, followed by a totals line. Within a phase,
line order can vary because independent tasks run in parallel.

```
  dotfiles v0.1.317
  profile  desktop · Arch Linux

:: Preparing dotfiles
  ~ Install wrapper
  ~ Configure PATH

:: Refreshing dotfiles
  ✓ Configure sparse checkout
  ○ Update repository — local changes present
  ~ Install Git hooks
  ✓ Reload configuration
  ~ Install shell completions

:: Applying configuration
  ~ Install symlinks
  ~ Install packages
  ~ Configure systemd units

  ~ 2 ok · 1 skipped · 7 dry-run · 6 not applicable
  completed in 1.3s
```

## Parallel Execution

The task pipeline has phase barriers: Bootstrap completes before Sync, Sync
completes before Provision, and Provision completes before the optional Update
phase. Within each phase, independent tasks run in parallel as soon as their
dependencies complete. Inside a task, resource operations (symlinks, packages,
registry entries, etc.) also run in parallel by default using Rayon's thread
pool.

**When parallel execution runs:**
- Independent tasks in the same phase can overlap
- Multiple symlinks are created concurrently
- Package state checks overlap
- Registry entries are applied in parallel

**Parallel execution is safe** — task dependencies are enforced by the scheduler,
and each resource is checked and applied independently.

To disable parallel execution, see [Advanced Binary Options](#advanced-binary-options).

## Dry-Run Mode

Preview what would be done without making changes:

```bash
./dotfiles.sh install -d
```

**Dry-run mode:**
- Shows all operations that would be performed
- Doesn't modify system state
- Useful for testing configuration changes
- Safe to run without privileges

Combine with `-v` for full detail on every resource:

```bash
./dotfiles.sh install -d -v
```

## Logging

All operations are logged to persistent log files. Use `dotfiles logs` (or
`./dotfiles.sh logs`) to print the most recent operation log. Use
`dotfiles logs -v` to print the diagnostic log when one is available.

**Linux:**
- Location: `${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.log`
- Includes: Timestamps, operations, full detail, summary

**Windows:**
- Location: `%USERPROFILE%\.cache\dotfiles\install.log`
- Includes: Timestamps, operations, full detail, summary

**Log contents:**
- Installation timestamp
- Selected profile
- Structured level and task context for all operations performed
- Full verbose-level detail (always, regardless of console verbose flag)
- Summary statistics
- Error messages and warnings

A **diagnostic log** is also written alongside the main log:

**Linux:**
- Location: `${XDG_CACHE_HOME:-$HOME/.cache}/dotfiles/install.diag.log`

**Windows:**
- Location: `%USERPROFILE%\.cache\dotfiles\install.diag.log`

The diagnostic log captures every event with sequence numbers,
microsecond-precision timestamps, and task context, preserving the true
chronological order of parallel execution. See
[Troubleshooting](TROUBLESHOOTING.md#using-diagnostic-logs) for details on
reading the diagnostic log.

If a command fails, the CLI may print `Run 'dotfiles logs' for details.` after
the actionable error so you can inspect the full log without routine successful
runs repeating the log path.

## Installation Summary

After installation, a summary is displayed. In **non-verbose** mode (default),
compact task-result lines are shown inline as task buffers flush, followed by
totals.

In **verbose** mode (`-v`), a full per-task breakdown grouped by domain is
appended. This is a logical summary, not the chronological execution order.

**Example (verbose):**
```
:: Summary
   Core
     ~ Install wrapper
     ~ Configure PATH
   Repository
     ✓ Configure sparse checkout
     ○ Update repository (local changes present)
     ✓ Reload configuration
   Git
     ~ Install Git hooks
   Files
     ~ Install symlinks
   Packages
     ~ Install packages
   System
     ~ Configure systemd units

  ~ 2 ok · 1 skipped · 6 dry-run · 6 not applicable
  completed in 1.3s
```

**Status icons:**
- `✓` — task completed successfully (green)
- `○` — deliberately skipped (yellow)
- `~` — dry-run preview (magenta)
- `✗` — task failed (red)

Not-applicable tasks are omitted from the summary display.

## Idempotency

All operations are idempotent - safe to run multiple times:

```bash
# First run
./dotfiles.sh install -p desktop
# Installs packages, creates symlinks, etc.

# Second run
./dotfiles.sh install
# Skips already-installed packages
# Skips existing symlinks
# Only performs necessary work
```

**Expected behavior:**
- No errors on repeated runs
- Operations logged as "Skipping: already correct"
- No duplicate installations
- System state remains consistent

## Profile Persistence

Your profile selection is automatically saved:

```bash
# First run with explicit profile
./dotfiles.sh install -p desktop
# Profile is saved to .git/config

# Subsequent runs
./dotfiles.sh install
# Uses saved profile automatically, no need to specify
```

**Manual profile management:**
```bash
# Check saved profile
git config --local --get dotfiles.profile

# Change saved profile
git config --local dotfiles.profile base
```

## Overlay Repository

An overlay repository provides private, additional configuration that is
merged with the main dotfiles config.  This is useful for work machine
configuration that should not be checked into a public dotfiles repo.

**Setting the overlay path:**
```bash
# Via CLI flag
./dotfiles.sh install --overlay /path/to/overlay

# Via environment variable
export DOTFILES_OVERLAY=/path/to/overlay
./dotfiles.sh install
```

**Windows:**
```powershell
.\dotfiles.ps1 install --overlay C:\Code\dotfiles-private
```

The overlay path is persisted in `dotfiles.overlay` git config, so you only
need to specify `--overlay` once:

```bash
# First run: specify overlay
./dotfiles.sh install --overlay ~/dotfiles-work

# Subsequent runs: overlay is remembered
./dotfiles.sh install
```

**What an overlay can provide:**
- **TOML config files** in `conf/` — merged with main config (packages,
  symlinks, extensions, etc.)
- **Custom scripts** in `scripts/` — defined in `conf/scripts.toml` with a
  convention-based interface (`--check`, `--dryrun`, `--remove`, no args for apply)

Each overlay script appears as its own task in the output. It is scheduled in
the Provision phase with other Provision tasks, so its relative position can
vary unless dependencies constrain it:

```
:: Refreshing dotfiles
  ✓ Reload configuration
  ✓ Load overlay scripts

:: Applying configuration
  ✓ Install symlinks
  ✓ Install private files   ← overlay script task
  ✓ Install packages
```

See [Configuration Reference](CONFIGURATION.md#overlay-configuration) for
the overlay file format.

## Examples by Use Case

### Minimal Server (Arch Linux)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p base
# Core configs + Arch packages (auto-detected), no desktop
```

### Full Desktop Workstation (Arch Linux)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Everything including desktop environment
```

### Non-Arch Linux Desktop (Ubuntu, Fedora, etc.)
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p desktop
# Desktop tools without Arch-specific packages
```

### Cross-Platform Development (Windows)
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
# Windows configurations, registry, desktop tools
```

### CI/Testing Environment
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install -p base -d
# Test configuration without system modifications
```

### Development (building from source)
```bash
./dotfiles.sh --build install -p base -d
# Builds the Rust binary from source, then runs it
```

## Advanced Binary Options

The following flags are available via the wrapper scripts or the binary directly.
The wrappers forward all arguments unchanged to the binary, so these work with
`dotfiles.sh` / `dotfiles.ps1` as well.

```bash
./dotfiles.sh install --skip packages,fonts
./dotfiles.sh install --only symlinks
./dotfiles.sh --no-parallel install
```

- **`--skip TASKS`** - Skip specific tasks (comma-separated)
- **`--only TASKS`** - Run only specific tasks (comma-separated)
- **`--overlay DIR`** - Use a private overlay repository
- **`--root DIR`** - Override dotfiles root directory (set automatically by wrapper scripts)
- **`--no-parallel`** - Disable task-level and resource-level parallel execution

## Shell Completions

Tab completions for the `dotfiles` CLI are generated automatically during
the **Sync** phase of every `install` run (Linux only).  The generated
script is written to:

```
symlinks/config/zsh/completions/_dotfiles
```

Because `~/.config/zsh/completions` is a symlink managed by the dotfiles
setup itself, the completions become available to zsh as soon as they are
written — no manual steps are required after `install`.

To regenerate completions without running a full install:

```bash
./dotfiles.sh install --only completions
```

To print a completion script for a specific shell without installing:

```bash
dotfiles completions zsh   # or: bash, fish, elvish, powershell
```

## See Also

- [Profile System](PROFILES.md) - Understanding profiles in detail
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Troubleshooting](TROUBLESHOOTING.md) - Common issues and solutions
- [Windows Usage](WINDOWS.md) - Windows-specific details
