# Dotfiles ✨

A personal, cross-platform dotfiles setup — and a **reference implementation** you're encouraged to fork, study, and remix into your own.

This repository is my own working dotfiles, but it's structured so the engine, conventions, and patterns can be lifted into your setup. Take what you like, throw out what you don't, and make it yours.

[![Release](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml)
[![Publish Docker](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker.yml)
[![CI](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml)

## Why fork this?

Most dotfiles repos are either *just config files* or *a tangle of bash glue*. This one tries to be a solid **chassis** you can build your own setup on:

- 🦀 **Rust core engine** — a single fast, typed, well-tested binary handles all orchestration. No fragile bootstrap scripts to debug at 2 AM.
- 🎯 **Profile-based** — one repo, many machines. Pick `base` for a minimal server or `desktop` for a full workstation; auto-detects Linux / Arch / Windows on top.
- 🔗 **Sparse checkout** — only the files relevant to the active profile land on disk.
- 📦 **Declarative TOML config** — packages, symlinks, systemd units, VS Code extensions, git config, registry keys, file permissions — all in `conf/*.toml`, no imperative scripts.
- 🔄 **Idempotent** — every command converges to the declared state. Safe to re-run, safe to dry-run (`-d`).
- 🪟 **Cross-platform for real** — Linux *and* Windows, from the same config, same binary.
- 📡 **Self-updating** — entry scripts pull the latest binary from GitHub Releases automatically.
- 🧪 **Tested in CI** — `cargo test`, `clippy`, `shellcheck`, and per-profile integration tests run on every change.
- 🐳 **Disposable Docker sandbox** to try things without touching your host.

If you've ever wanted dotfiles that feel more like a small piece of well-engineered software than a pile of shell scripts, this is meant to be a starting point.

## Try it in 30 seconds (no commitment)

Spin up the Docker image to poke around without touching your host:

```bash
docker run --rm -it sneivandt/dotfiles
```

## Fork and make it yours

1. **Fork the repo** (or use "Use this template") and clone it.
2. **Rebrand it.** A handful of files still reference the original owner — repo slug for self-update, git identity, CI fixtures, Docker labels.
   - **With GitHub Copilot in VS Code:** run the [`fork-rebrand`](.github/prompts/fork-rebrand.prompt.md) prompt and let an agent do it for you.
   - **Manually:** grep for `sneivandt` and replace each hit with your own owner / repo / image name; update name and email in `symlinks/config/git/config`.
3. **Make it yours.** Drop your own dotfiles into `symlinks/`, then edit `conf/*.toml` to declare what should be installed and where (see [Configuration](#configuration) and [Profiles](#profiles)).
4. **Verify and ship.** Run `cd cli && cargo test`, then push a tag — CI builds Linux + Windows binaries so your other machines can bootstrap from a `curl | sh`-style flow.

You don't have to keep the Rust engine — but if you do, you mostly just edit TOML and drop files into `symlinks/`. Adding a new tool is usually a one-line config change, not a new shell function.

### Things worth stealing even if you don't fork the whole thing

- The `Resource` / `Applicable` trait pattern in `cli/src/resources/` for idempotent system operations.
- The `resource_task!` / `task_deps!` macros for declarative task graphs.
- The category-based AND-matching scheme (`[arch-desktop]`, `[linux]`) for cross-platform config.
- The sparse-checkout + manifest pattern for slimming down what hits each machine.
- The GitHub Copilot agent skills in `.github/skills/` — domain-specific guidance that makes AI-assisted edits actually safe.

## How it works

Three layers, by design:

1. **Entry scripts** (`dotfiles.sh`, `dotfiles.ps1`) — thin wrappers. They download the latest binary from your GitHub Releases (or build from source with `--build`) and forward all arguments.
2. **Rust binary** (`cli/`) — does all the real work: config parsing, profile resolution, symlinks, file permissions, package management. Shells out only when it must (package managers, systemd).
3. **Configuration** (`conf/`) — declarative TOML. This is the part you'll edit most.

Binary updates happen automatically after the first bootstrap. On Windows, the PowerShell wrapper additionally promotes any staged update before relaunching.

## Quick start (using this repo as-is)

### Linux
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh install  # Prompts for profile selection
```

### Windows
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1 install -p desktop
```

## Usage

```bash
# Install with profile
./dotfiles.sh install -p desktop

# Preview changes (dry-run)
./dotfiles.sh install -d

# Verbose output
./dotfiles.sh install -v

# Uninstall (remove symlinks)
./dotfiles.sh uninstall

# Run validation tests
./dotfiles.sh test

# Print version
./dotfiles.sh version

# Build and run from source (development)
./dotfiles.sh --build install -p base
```

For detailed usage, see the [Usage Guide](docs/USAGE.md).

## Profiles

Profiles control which files are included and which components are installed:

| Profile | Description |
|---------|-------------|
| `base` | Minimal core shell configuration |
| `desktop` | Full configuration including desktop tools |

Platform categories (`linux`, `windows`, `arch`) are auto-detected based on the running OS. In your fork, define whatever profiles match your fleet — `work`, `server`, `vm`, `gaming`, etc.

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Configuration is defined in `conf/*.toml` files using TOML format:

- **`profiles.toml`** - Profile definitions
- **`manifest.toml`** - File-to-category mappings for sparse checkout
- **`symlinks.toml`** - Files to symlink to `$HOME`
- **`packages.toml`** - System packages to install
- **`systemd-units.toml`** - Systemd units to enable
- **`vscode-extensions.toml`** - VS Code extensions
- **`copilot-plugins.toml`** - GitHub Copilot CLI plugins
- **`git-config.toml`** - Git configuration settings
- **`registry.toml`** - Windows registry settings
- **`chmod.toml`** - File permissions

See the [Configuration Reference](docs/CONFIGURATION.md) for detailed format documentation.

## Development

```bash
# Build the Rust binary
cd cli && cargo build

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt

# Run from source
./dotfiles.sh --build install -p base -d
```

See [Contributing](docs/CONTRIBUTING.md) for development guidelines.

## Testing

```bash
# Rust tests (unit + integration)
cd cli && cargo test

# Validate configuration
./dotfiles.sh test
```

The CI workflow validates: `cargo fmt`, `cargo clippy`, `cargo test`, build on Linux and Windows, shellcheck on wrapper scripts, integration tests per profile.

See [Testing Documentation](docs/TESTING.md) for details.

## Docker

```bash
docker run --rm -it sneivandt/dotfiles
docker buildx build -t dotfiles:local .
```

Published image: [`sneivandt/dotfiles`](https://hub.docker.com/r/sneivandt/dotfiles)

## Documentation

- **[Usage Guide](docs/USAGE.md)** - Installation and usage instructions
- **[Profile System](docs/PROFILES.md)** - Understanding and using profiles
- **[Configuration Reference](docs/CONFIGURATION.md)** - Configuration file formats
- **[Architecture](docs/ARCHITECTURE.md)** - Rust engine design and structure
- **[Contributing](docs/CONTRIBUTING.md)** - Development workflow and guidelines
- **[Testing](docs/TESTING.md)** - Testing procedures and CI
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[Windows Usage](docs/WINDOWS.md)** - Windows-specific documentation
- **[Docker](docs/DOCKER.md)** - Docker image usage and building
- **[Git Hooks](docs/HOOKS.md)** - Repository git hooks
- **[Security](docs/SECURITY.md)** - Security policy and best practices
