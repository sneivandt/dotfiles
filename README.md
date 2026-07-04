# Dotfiles

A personal dotfiles manager built around a **Rust CLI** and declarative TOML configuration. It keeps my Linux and Windows environments consistent across shell, editor, Git, packages, services, system settings, and AI tooling.

![Generated terminal preview of a dotfiles dry-run install](docs/assets/terminal-screenshot.svg)

## Core ideas

- **Cross-platform:** one Rust CLI plans and applies the desired machine state across Linux and Windows.
- **Profile-aware:** select `base` for minimal environments or `desktop` for workstations; the CLI adds the matching platform categories for the current system.
- **Declarative:** TOML files describe packages, links, tools, and settings without turning setup into a collection of one-off scripts.
- **Idempotent:** re-running `install` converges on the declared state. Preview changes first with `-d`.

## Commands

Bootstrap with the platform wrapper: `./dotfiles.sh install` on Linux or
`.\dotfiles.ps1 install` on Windows. The wrapper downloads the latest release
when no binary is present; add `--build` to compile from source instead. After
bootstrap, use the installed `dotfiles` command.

| Task | Command |
|------|---------|
| Apply config | `dotfiles install` |
| Preview changes | `dotfiles install -d` |
| Update dependencies | `dotfiles update` |
| Detach managed files | `dotfiles uninstall` |
| Validate config | `dotfiles test` |
| Inspect logs | `dotfiles log` |
| Show version | `dotfiles version` |

Use `install` for normal, repeatable convergence. Use `update` when you also
want to advance pinned dependency versions. Use `uninstall` only to detach
managed links/hooks/wrappers: managed symlinks are replaced with real files or
directories copied from their current sources, and broader machine state is not
reverted.

See the [Usage Guide](docs/USAGE.md) for the full command reference.

## Profiles

Each machine uses one profile; platform categories are detected automatically.

| Profile | Best for |
|---------|----------|
| `base` | Servers, WSL, minimal shell environments |
| `desktop` | Full desktop/workstation setups with GUI tools |

The `linux`, `windows`, and `arch` categories are detected automatically and combined with the selected profile.

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Declarative settings are stored in `conf/*.toml`. Edit these files and the CLI applies the requested state:

| File | Controls |
|------|----------|
| `profiles.toml` | Profile definitions |
| `manifest.toml` | Sparse-checkout file-to-category mappings |
| `symlinks.toml` | Files linked into `$HOME` |
| `packages.toml` | Packages for pacman, AUR, or winget |
| `systemd-units.toml` | systemd units to enable |
| `vscode-extensions.toml` | VS Code extensions |
| `git-config.toml` | Git settings |
| `registry.toml` | Windows registry keys |
| `chmod.toml` | File permissions |

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
| [Profile System](docs/PROFILES.md) | How profiles and categories work |
| [Configuration Reference](docs/CONFIGURATION.md) | TOML format details |
| [Architecture](docs/ARCHITECTURE.md) | Rust CLI design |
| [Contributing](docs/CONTRIBUTING.md) | Development workflow |
