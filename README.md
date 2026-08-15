# Dotfiles

A personal dotfiles manager for Linux and Windows, powered by a **Rust CLI**
and declarative TOML.

![Generated terminal output of a dotfiles install](docs/assets/terminal-screenshot.svg)

## Quick start

From a repository checkout, preview the changes before applying them:

**Linux**

```bash
./dotfiles.sh install --profile base --dry-run
./dotfiles.sh install --profile base
```

**Windows**

```powershell
.\dotfiles.ps1 install --profile desktop --dry-run
.\dotfiles.ps1 install --profile desktop
```

Each wrapper downloads and verifies the latest compatible release binary when
needed. Pass `--build` to compile from source. After installation, use the
`dotfiles` command directly.

## Core ideas

- **Cross-platform:** one Rust CLI plans and applies the desired machine state across Linux and Windows.
- **Adaptive:** select `base` or `desktop`; platform-specific settings are added automatically.
- **Declarative:** TOML describes packages, links, tools, and settings.
- **Idempotent:** repeated installs apply only the changes needed to match the configuration.

## What it manages

The selected profile and detected platform determine which entries apply.

| Area | Managed state |
|------|---------------|
| Shell | Zsh and Bash configuration, login shell, `PATH`, and completions |
| Editors | Neovim, Vim, VS Code, and VS Code extensions |
| Terminal | Alacritty, tmux, and readline configuration |
| Git | Global settings and repository hooks |
| Packages | pacman and AUR packages via `paru` on Arch; winget packages on Windows |
| Linux desktop | Hyprland, Waybar, mako, fuzzel, gammastep, and GTK configuration |
| Services | systemd user units and maintenance timers |
| SSH and GnuPG | Configuration files with enforced Unix file modes |
| AI tooling | APM packages and plugins, plus Copilot and Codex settings |
| Windows | Current-user registry values, Developer Mode, and WSL configuration |

See the [Task Reference](docs/TASKS.md) for the tasks behind these areas.

## Commands

| Command | What it does |
|---------|--------------|
| `dotfiles install` | Brings the machine in line with the configuration |
| `dotfiles update` | Installs and advances pinned dependency versions |
| `dotfiles uninstall` | Removes managed integrations while preserving user files |
| `dotfiles test` | Validates configuration and runs available script analyzers |
| `dotfiles tasks` | Lists task selectors and which commands use each task |
| `dotfiles log` | Reads retained run logs |

Use `install` for normal setup and maintenance, and `update` only when advancing
pinned versions. `uninstall` leaves packages, services, and registry values in
place. Commands that make changes accept `-d, --dry-run` to report changes
without applying them.

### Targeting specific tasks

Use `--only` and `--skip` with selectors reported by `dotfiles tasks`:

```bash
dotfiles tasks
dotfiles install --only symlinks,git --dry-run
dotfiles install --skip packages
```

See the [Usage Guide](docs/USAGE.md) for the full command reference.

## Profiles

Each machine uses one profile. The CLI adds the detected `linux`, `windows`, and
`arch` categories automatically.

```bash
dotfiles install -p desktop
```

If no profile is set, `install` prompts for one and saves the choice.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Workstations with GUI tools |

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Configuration lives in `conf/*.toml`. Key files include:

| File | Controls |
|------|----------|
| `profiles.toml` | Profile definitions |
| `manifest.toml` | Files included for each profile/platform |
| `symlinks.toml` | Files linked into `$HOME` |
| `packages.toml` | Packages for pacman, AUR, or winget |
| `git-config.toml` | Git settings |
| `agent-settings.toml` | Copilot and Codex user settings |
| `registry.toml` | Windows registry keys |

See the [Configuration Reference](docs/CONFIGURATION.md) for every file and its
TOML format.

## Development

Run Rust commands from `cli/`:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
```

From the repository root:

```bash
./dotfiles.sh --build install --dry-run
```

See the [Contributing Guide](docs/CONTRIBUTING.md) for development workflows.

## Documentation

| Guide | What's in it |
|-------|--------------|
| [Documentation index](docs/README.md) | All user, platform, and development guides |
| [Usage Guide](docs/USAGE.md) | All commands and flags |
| [Task Reference](docs/TASKS.md) | Every install, update, uninstall, validation, and overlay task |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust CLI design |
| [Troubleshooting](docs/TROUBLESHOOTING.md) | Common setup and configuration failures |
