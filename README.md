# Dotfiles

My personal dotfiles manager built around a **Rust CLI** and declarative TOML configuration. It keeps my Linux and Windows environments consistent across shell, editor, Git, packages, AI tooling and more.

![Generated terminal preview of a dotfiles install](docs/assets/terminal-screenshot.svg)

## Core ideas

- **Cross-platform:** one Rust CLI plans and applies the desired machine state across Linux and Windows.
- **Adaptive:** one repository serves every machine. Choose `base` or `desktop`; the `linux`, `windows`, and `arch` categories are detected and layered on automatically.
- **Declarative:** TOML files describe packages, links, tools, and settings without turning setup into a collection of one-off scripts.
- **Idempotent:** re-running `install` converges on the declared state. Preview changes first with `-d`.

## What it manages

The active profile and detected platform decide which of these apply on a given
machine, so nothing below is installed everywhere.

| Area | Managed state |
|------|---------------|
| Shell | Zsh and Bash configuration, login shell selection, `PATH` entry, generated completions |
| Editors | Neovim, Vim, and VS Code configuration plus declared VS Code extensions |
| Terminal | Alacritty, tmux, and readline configuration |
| Git | Global Git settings and repository-maintained hooks |
| Packages | pacman and AUR packages via `paru` on Arch, winget packages on Windows |
| Linux desktop | Hyprland, Waybar, mako, fuzzel, gammastep, and GTK settings |
| Services | systemd user units for desktop components and maintenance timers |
| Sensitive files | SSH and GnuPG configuration, with declared Unix file modes enforced |
| AI tooling | APM packages and plugins, and Copilot CLI settings |
| Windows | Current-user registry values, Developer Mode, and WSL configuration |

See the [Task Reference](docs/TASKS.md) for the task behind each area.

## Commands

Bootstrap with the platform wrapper from the repository root: `./dotfiles.sh` on
Linux or `.\dotfiles.ps1` on Windows. The wrapper downloads the latest release
binary when none is present; add `--build` to compile from source instead. After
the first run, the installed `dotfiles` command is the normal entry point.

Every command accepts `-d, --dry-run`, which plans and reports changes without
touching the machine. Preview a first run before applying it:

```bash
./dotfiles.sh install -p desktop -d   # preview
./dotfiles.sh install -p desktop      # apply
```

These commands change machine state:

| Command | What it does |
|---------|--------------|
| `dotfiles install` | Converges the machine on the declared state |
| `dotfiles update` | Runs `install` and additionally advances pinned dependency versions |
| `dotfiles uninstall` | Detaches managed links, hooks, and wrappers, replacing symlinks with real files |

Reach for `update` only when you want to advance pinned versions; `install` is
the command for normal repeatable convergence. `uninstall` leaves broader
machine state such as packages, services, and registry values alone.

These commands only read state:

| Command | What it does |
|---------|--------------|
| `dotfiles test` | Validates configuration and runs available script analyzers |
| `dotfiles tasks` | Lists task selectors and the commands that include each one |
| `dotfiles log` | Lists retained run logs or prints one of them |

### Targeting specific tasks

`--only` and `--skip` narrow a run to a subset of tasks, using the stable
selectors reported by `dotfiles tasks`:

```bash
dotfiles tasks                          # list selectors
dotfiles install --only symlinks,git -d # preview only those tasks
dotfiles install --skip packages        # converge everything else
```

See the [Usage Guide](docs/USAGE.md) for the full command reference.

## Profiles

Each machine uses one profile; `linux`, `windows`, and `arch` are detected automatically and combined with the selected profile. Select a profile with `-p, --profile`:

```bash
dotfiles install -p desktop
```

If no profile is set, `install` prompts for one and saves the selection for future runs.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Full desktop/workstation setups with GUI tools |

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Declarative settings are stored in `conf/*.toml`. Edit these files and the CLI applies the requested state. The table below highlights the core configuration files; it is not a complete list:

| File | Controls |
|------|----------|
| `profiles.toml` | Profile definitions |
| `manifest.toml` | Files included for each profile/platform |
| `symlinks.toml` | Files linked into `$HOME` |
| `packages.toml` | Packages for pacman, AUR, or winget |
| `git-config.toml` | Git settings |
| `registry.toml` | Windows registry keys |

See the [Configuration Reference](docs/CONFIGURATION.md) for the full TOML format.

## Development

Run Rust development commands from the `cli/` directory:

```bash
cargo build                      # build
cargo test                       # unit + integration tests
cargo clippy -- -D warnings      # lint
cargo fmt                        # format
```

From the repo root, build from source and preview changes against the active config:

```bash
./dotfiles.sh --build install -d # run from repo root
```

## Documentation

| Guide | What's in it |
|-------|--------------|
| [Usage Guide](docs/USAGE.md) | All commands and flags |
| [Task Reference](docs/TASKS.md) | Every install, update, uninstall, validation, and overlay task |
| [Profile System](docs/PROFILES.md) | How profiles work |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust CLI design |
| [APM Tooling](docs/APM.md) | AI tooling packages and APM flow |