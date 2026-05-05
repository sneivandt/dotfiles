# Dotfiles ✨

[![Release](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml)
[![Publish Docker](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml)
[![CI](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml)

A cross-platform dotfiles manager built on a **Rust engine** with declarative TOML configuration. This is my personal setup, designed as a **reference chassis**. Fork it, rebrand it, and make it yours.

![Generated terminal preview of a dotfiles dry-run install](docs/assets/terminal-screenshot.svg)

## What you get

- 🦀 **Rust engine:** a single compiled binary orchestrates everything; no fragile shell pipelines to debug
- 🎯 **Profile-based:** one repo, many machines; pick `base` for a minimal shell or `desktop` for a full workstation; Linux / Arch / Windows is auto-detected on top
- 📦 **Declarative TOML:** packages, symlinks, systemd units, VS Code extensions, git config, registry keys, and file permissions are all one-liners in `conf/*.toml`, no new shell functions
- 🔄 **Idempotent:** every run converges to the declared state; safe to re-run anytime, preview with `-d`
- 🔗 **Sparse checkout:** only files relevant to the active profile ever land on disk
- 📡 **Self-updating:** entry scripts download the latest binary from GitHub Releases automatically; you never manage the binary manually
- 🪟 **Truly cross-platform:** Linux and Windows share the same config and the same binary
- 🧪 **CI tested:** `cargo test`, `clippy`, `shellcheck`, and per-profile integration tests on every push
- 🐳 **Docker sandbox:** try the full setup without touching your host

## Try it first

Spin up the Docker image to explore before installing anything:

```bash
docker run --rm -it sneivandt/dotfiles
```

## Install

**Prerequisites:** git. Rust is only needed if using `--build` to compile from source.

### Agent-first (recommended)

Fork and set up this repo as your own by giving your AI agent (GitHub Copilot, Claude, Cursor, etc.) this prompt:

```
Adopt https://github.com/sneivandt/dotfiles as my personal dotfiles repo.
GitHub username: [YOUR_USERNAME], git name: [YOUR_NAME], git email: [YOUR_EMAIL].

1. Fork and clone it.
2. Run .github/prompts/fork-rebrand.prompt.md to rebrand it to my repo.
3. Ask me what I want installed and configured, then update symlinks/ and conf/*.toml.
4. Install: ./dotfiles.sh install -p desktop  (or .\dotfiles.ps1 on Windows)
```

### Manual install

**Linux:**
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install        # interactive profile prompt on first run
```

**Windows (PowerShell):**
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

## Common commands

**Linux:**
```bash
./dotfiles.sh install -p desktop   # install with explicit profile
./dotfiles.sh install -d           # dry-run (preview without applying)
./dotfiles.sh install -v           # verbose output
./dotfiles.sh uninstall            # remove symlinks
./dotfiles.sh test                 # validate configuration
./dotfiles.sh version              # print binary version
./dotfiles.sh --build install -d   # build from source and dry-run
```

**Windows (PowerShell):**
```powershell
.\dotfiles.ps1 install -p desktop
.\dotfiles.ps1 install -d
```

See the [Usage Guide](docs/USAGE.md) for the full reference.

## Profiles

Choose one profile per machine. Platform categories are detected automatically.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Workstations with GUI tools (Arch: Hyprland/Wayland) |

The `linux`, `windows`, and `arch` platform categories are auto-detected and layer on top of whichever profile you pick. In your own fork you can define any profiles you like: `work`, `server`, `vm`, `gaming`, etc.

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Everything declarative lives in `conf/*.toml`:

| File | Controls |
|------|----------|
| `profiles.toml` | Profile definitions |
| `manifest.toml` | Sparse-checkout file-to-category mappings |
| `symlinks.toml` | Files symlinked into `$HOME` |
| `packages.toml` | System packages (pacman, AUR, winget) |
| `systemd-units.toml` | Systemd units to enable |
| `vscode-extensions.toml` | VS Code extensions |
| `git-config.toml` | Git settings |
| `registry.toml` | Windows registry keys |
| `chmod.toml` | File permissions |

See the [Configuration Reference](docs/CONFIGURATION.md) for the full TOML format.

## How it works

Three layers, kept deliberately thin:

1. **Entry scripts** (`dotfiles.sh` / `dotfiles.ps1`): download the binary from GitHub Releases (or build with `--build`) and forward arguments.
2. **Rust binary** (`cli/`): parses config, resolves profiles, applies symlinks, packages, and settings. Shells out only when it must (package managers, systemd).
3. **Configuration** (`conf/`): the part you edit. Everything else follows from the TOML.

## Making it yours

The [`fork-rebrand`](.github/prompts/fork-rebrand.prompt.md) agent prompt automates most of this. Or follow the steps manually:

1. **Fork** the repo (or "Use this template") and clone it.
2. **Rebrand:** update the self-update URL, git identity, CI fixture, and Docker labels to point at your repo. The `fork-rebrand` prompt handles all of this automatically. Manually: replace all occurrences of `sneivandt` in the repo and update your name and email in `symlinks/config/git/config`.
3. **Add your dotfiles:** drop your config files into `symlinks/`. Whatever is there gets symlinked into `$HOME` on install.
4. **Declare your tools:** edit `conf/*.toml` to list the packages, extensions, and settings you want on each machine.
5. **Push a tag:** CI builds Linux + Windows binaries; any new machine can bootstrap from a single command.

## Development

Run all commands from the `cli/` directory:

```bash
cargo build                      # build
cargo test                       # unit + integration tests
cargo clippy -- -D warnings      # lint
cargo fmt                        # format
```

To build from source and preview changes against your actual config:

```bash
./dotfiles.sh --build install -d # run from repo root
```

See [Contributing](docs/CONTRIBUTING.md) for the full development workflow.

## Documentation

| Guide | What's in it |
|-------|--------------|
| [Usage Guide](docs/USAGE.md) | All commands and flags |
| [Profile System](docs/PROFILES.md) | How profiles and categories work |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust engine design |
| [Contributing](docs/CONTRIBUTING.md) | Development workflow |
| [Testing](docs/TESTING.md) | Test strategy and CI |
| [Troubleshooting](docs/TROUBLESHOOTING.md) | Common issues and fixes |
| [Windows Usage](docs/WINDOWS.md) | Windows-specific notes |
| [Docker](docs/DOCKER.md) | Sandbox usage |
| [Git Hooks](docs/HOOKS.md) | Repository hooks |
| [Security](docs/SECURITY.md) | Security policy |
