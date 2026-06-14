# Dotfiles

[![Release](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml)
[![Publish Docker](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml)
[![CI](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml)

A cross-platform dotfiles manager powered by a **Rust engine** and declarative TOML configuration. It's the setup I run every day on Linux and Windows, and it's designed to be forked: rebrand it, swap in your own config, and make it yours.

![Generated terminal preview of a dotfiles dry-run install](docs/assets/terminal-screenshot.svg)

## What you get

- **Rust engine:** a single compiled binary orchestrates everything — no fragile shell pipelines to debug.
- **Profile-based:** one repo, many machines. Pick `base` for a minimal shell or `desktop` for a full workstation; Linux, Arch, and Windows are auto-detected on top.
- **Declarative TOML:** packages, symlinks, systemd units, VS Code extensions, git config, registry keys, and file permissions are each a one-liner in `conf/*.toml` — no new shell functions to write.
- **Idempotent:** every run converges on the declared state, so it's safe to re-run anytime. Preview first with `-d`.
- **Sparse checkout:** only the files relevant to your active profile ever land on disk.
- **Self-updating:** the entry scripts fetch the latest binary from GitHub Releases for you — there's nothing to install or upgrade by hand.
- **Cross-platform:** Linux and Windows share the same config and the same binary.
- **CI tested:** `cargo test`, `clippy`, `shellcheck`, and per-profile integration tests run on every push.
- **Docker sandbox:** explore the full setup in a throwaway container without touching your host.

## Try it first

Run a full install inside a disposable container — nothing touches your host:

```bash
docker run --rm -it sneivandt/dotfiles
```

## Install

**Prerequisites:** Git. Rust is only required if you compile from source with `--build`.

### Agent-first (recommended)

Hand this prompt to your AI agent (GitHub Copilot, Claude, Cursor, and friends) to fork the repo and tailor it to you in one go:

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
./dotfiles.sh update               # install + advance pinned dependency versions
./dotfiles.sh uninstall            # remove symlinks
./dotfiles.sh test                 # validate configuration
./dotfiles.sh logs                 # view the most recent operation log
./dotfiles.sh logs -v              # view the diagnostic log when available
./dotfiles.sh version              # print binary version
```

**Windows (PowerShell):**
```powershell
.\dotfiles.ps1 install -p desktop
.\dotfiles.ps1 install -d
.\dotfiles.ps1 logs
```

See the [Usage Guide](docs/USAGE.md) for the full reference.

## Profiles

Pick one profile per machine; the platform categories are detected for you.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Workstations with GUI tools (Arch: Hyprland/Wayland) |

The `linux`, `windows`, and `arch` platform categories are detected at runtime and layer on top of whichever profile you choose. In your own fork, define any profiles that fit your machines — `work`, `server`, `vm`, `gaming`, and so on.

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Everything declarative lives in `conf/*.toml`. Edit these files and the engine takes care of the rest:

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

1. **Entry scripts** (`dotfiles.sh` / `dotfiles.ps1`): download the binary from GitHub Releases (or build it with `--build`) and forward your arguments.
2. **Rust binary** (`cli/`): parses the config, resolves your profile, and applies symlinks, packages, and settings. It shells out only when it has to — package managers, systemd, and the like.
3. **Configuration** (`conf/`): the part you edit. Everything else follows from the TOML.

## Making it yours

The [`fork-rebrand`](.github/prompts/fork-rebrand.prompt.md) agent prompt automates most of this. To do it by hand:

1. **Fork** the repo (or "Use this template") and clone it.
2. **Rebrand:** point the self-update URL, git identity, CI fixture, and Docker labels at your own repo. The `fork-rebrand` prompt does this for you; manually, replace every occurrence of `sneivandt` and set your name and email in `symlinks/config/git/config`.
3. **Add your dotfiles:** drop your config files into `symlinks/`. Anything there is symlinked into `$HOME` on install.
4. **Declare your tools:** edit `conf/*.toml` to list the packages, extensions, and settings you want on each machine.
5. **Push a tag:** CI builds the Linux and Windows binaries, and any new machine can bootstrap from a single command.

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
