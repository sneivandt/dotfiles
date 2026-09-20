<h1 align="center">Dotfiles</h1>

<p align="center">
  <strong>My Arch Linux and Windows setup, managed by a Rust CLI.</strong>
</p>

<p align="center">
  <a href="https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml"><img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/sneivandt/dotfiles/ci.yml?branch=main&style=flat-square&label=CI"></a>
  <a href="https://github.com/sneivandt/dotfiles/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/sneivandt/dotfiles?style=flat-square&label=release"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-9ece6a?style=flat-square"></a>
</p>

Packages, symlinks, and system settings live in [conf/](conf/), with app
configuration under [symlinks/](symlinks/). The CLI selects entries for the
active profile and operating system, compares them with the machine, and
applies changes in dependency order. Settings that already match are skipped.

<p align="center">
  <img src="docs/assets/terminal-screenshot.svg" width="600" alt="Example install output showing changed symlinks, packages, and the default shell, followed by a summary">
</p>

## What it manages

| Area | Configuration |
|---|---|
| Shell | Zsh, Bash, PowerShell, tmux, PATH, and completions |
| Editors | Vim/Neovim, VS Code settings, and extensions |
| Terminal | Alacritty and Windows Terminal |
| Git | Global settings, aliases, and repository hooks |
| Packages | paru for Arch Linux packages and winget packages on Windows |
| Arch desktop | Hyprland, Quickshell, mako, fuzzel, and GTK settings |
| Services | systemd user and system units |
| AI tooling | APM packages and plugins, Copilot and Codex settings |
| Windows | Current-user registry values and Developer Mode |

The `base` profile covers shell tools and core configuration for servers and
WSL. The `desktop` profile adds GUI apps and desktop services. Both profiles
work on Linux and Windows.

## Usage

The wrapper scripts download and verify the CLI when needed. Pass `--build`
to build from source.

On Linux:

```sh
./dotfiles.sh install --profile base --dry-run
```

On Windows:

```powershell
.\dotfiles.ps1 install --profile desktop --dry-run
```

After installation, `dotfiles` is available directly in a new shell.

| Command | What it does |
|---|---|
| `dotfiles install` | Syncs the repository and applies the selected configuration |
| `dotfiles update` | Installs the configuration and updates pinned dependencies |
| `dotfiles uninstall` | Replaces managed symlinks with local copies and removes managed hooks and the launcher |
| `dotfiles check` | Checks configuration and runs available script analyzers |
| `dotfiles log` | Shows saved run logs |

Use `--dry-run` to preview changes, `--only <selector>` to select tasks, and
`--no-repo-update` to run against the current checkout without syncing it.
Uninstall leaves packages, services, and registry values in place.

## Documentation

- [Usage](docs/USAGE.md), [profiles](docs/PROFILES.md), and [task reference](docs/TASKS.md)
- [Configuration](docs/CONFIGURATION.md), including private overlays
- [Architecture](docs/ARCHITECTURE.md)
- [Development](docs/CONTRIBUTING.md) and [testing](docs/TESTING.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md) and [Windows](docs/WINDOWS.md)
