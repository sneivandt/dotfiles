# Dotfiles

A highly personal dotfiles manager powered by a **Rust engine** and declarative TOML configuration. This is the setup I run every day on Linux and Windows to converge my machines onto the tools, symlinks, editor settings, Git config, services, and system preferences I want.

It manages my shell, editor, Git, packages, services, Windows registry settings, file permissions, sparse checkout state, and AI tooling across Linux and Windows.

![Generated terminal preview of a dotfiles dry-run install](docs/assets/terminal-screenshot.svg)

## What it does

- **Rust engine:** a single compiled binary orchestrates everything — no fragile shell pipelines to debug.
- **Profile-based:** one repo, many machines. `base` covers a minimal shell, `desktop` covers a full workstation, and Linux, Arch, and Windows are auto-detected on top.
- **Declarative TOML:** packages, symlinks, systemd units, VS Code extensions, git config, registry keys, and file permissions are each a one-liner in `conf/*.toml` — no new shell functions to write.
- **Idempotent:** every run converges on the declared state, so it's safe to re-run anytime. Preview first with `-d`.
- **Sparse checkout:** only the files relevant to the active profile ever land on disk.
- **Cross-platform:** Linux and Windows share the same config and the same binary.

## Commands

Git is the only prerequisite for a normal bootstrap. The repo-level wrapper downloads the latest binary from GitHub Releases and runs the initial install; after that, the normal entry point is the installed `dotfiles` command. Rust is only required when compiling from source with `--build`.

Initial bootstrap uses the platform wrapper: `./dotfiles.sh install -p desktop` on Linux or `.\dotfiles.ps1 install -p desktop` on Windows.

| Task | Command |
|------|---------|
| Install | `dotfiles install -p desktop` |
| Dry run | `dotfiles install -d` |
| Update | `dotfiles update` |
| Uninstall managed links/hooks | `dotfiles uninstall` |
| Validate config | `dotfiles test` |
| View logs | `dotfiles logs` |
| Print version | `dotfiles version` |

See the [Usage Guide](docs/USAGE.md) for the full command reference.

## Profiles

Each machine uses one profile; platform categories are detected automatically.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Workstations with GUI tools (Arch: Hyprland/Wayland) |

The `linux`, `windows`, and `arch` platform categories are detected at runtime and layer on top of whichever profile is selected.

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

1. **Entry scripts** (`dotfiles.sh` / `dotfiles.ps1`): download the binary from GitHub Releases (or build it with `--build`) and forward arguments.
2. **Rust binary** (`cli/`): parses the config, resolves the profile, and applies symlinks, packages, and settings. It shells out only when it has to — package managers, systemd, and the like.
3. **Configuration** (`conf/`): the editable layer. Everything else follows from the TOML.

`install` is the normal convergence command: it may self-update the binary,
attempt a safe fast-forward repository sync, reload config, and apply declared
state without advancing pinned dependency versions. `update` runs the same flow
plus a final dependency-advancement phase. `uninstall` is conservative and only
detaches the managed symlinks, Git hooks, and wrapper; it does not remove
packages or roll back system/editor settings.

## Development

Run all commands from the `cli/` directory:

```bash
cargo build                      # build
cargo test                       # unit + integration tests
cargo clippy -- -D warnings      # lint
cargo fmt                        # format
```

To build from source and preview changes against the active config:

```bash
./dotfiles.sh --build install -d # run from repo root
```

## Documentation

| Guide | What's in it |
|-------|--------------|
| [Usage Guide](docs/USAGE.md) | All commands and flags |
| [Profile System](docs/PROFILES.md) | How profiles and categories work |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust engine design |
| [Contributing](docs/CONTRIBUTING.md) | Development workflow |
