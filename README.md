# Dotfiles

A personal dotfiles manager powered by a **Rust engine** and declarative TOML configuration. It converges my Linux and Windows machines onto the shell, editor, Git, packages, services, system preferences, sparse checkout state, and AI tooling I use every day.

![Generated terminal preview of a dotfiles dry-run install](docs/assets/terminal-screenshot.svg)

## What it does

- **Single Rust engine:** one compiled binary plans and applies changes, keeping the shell wrappers thin.
- **Profile-aware setup:** `base` covers minimal environments, `desktop` adds workstation tools, and Linux, Arch, and Windows layers are detected automatically.
- **Declarative configuration:** packages, symlinks, services, editor settings, Git config, registry keys, file permissions, and AI tooling all live in `conf/*.toml`.
- **Safe convergence:** re-running `install` brings the machine back to the declared state. Preview changes first with `-d`.
- **Sparse checkout support:** only files relevant to the active profile are checked out locally.
- **Cross-platform by design:** Linux and Windows use the same configuration model and Rust binary.

## Commands

Bootstrap with the platform wrapper: `./dotfiles.sh install` on Linux or `.\dotfiles.ps1 install` on Windows. The first run prompts for a profile and saves it. Add `--build` to compile from source; otherwise the wrapper downloads the latest release. After bootstrap, use the installed `dotfiles` command.

| Task | Command |
|------|---------|
| Install | `dotfiles install` |
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
| `desktop` | Full desktop/workstation setups with GUI tools |

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
| [Profile System](docs/PROFILES.md) | How profiles and categories work |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust engine design |
| [Contributing](docs/CONTRIBUTING.md) | Development workflow |
