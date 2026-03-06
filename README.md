# Dotfiles ✨

Cross-platform dotfiles management system powered by a Rust core engine, with profile-based configuration for Linux, Arch Linux, and Windows.

**Key Features:**
- 🦀 Rust core engine — fast, reliable, cross-platform binary
- 🎯 Profile-based configuration
- 🔗 Git sparse checkout for environment-specific files
- 📦 Declarative package and symlink management via TOML config
- 🔄 Idempotent installation (safe to re-run)
- 📡 Automatic binary updates from GitHub Releases
- 🤖 GitHub Copilot Agent Skills for development guidance
- 🧪 Comprehensive Rust test suite and CI
- 🐳 Docker image for isolated testing

[![Release](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/release.yml)
[![Publish Docker](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)
[![CI](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml)

## Quick Start

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

### Docker
```bash
docker run --rm -it sneivandt/dotfiles
```

## How It Works

The dotfiles system has three layers:

1. **Entry scripts** (`dotfiles.sh`, `dotfiles.ps1`) — thin wrappers that download the latest binary from GitHub Releases (or build from source with `--build`) and forward all arguments.
2. **Rust binary** (`cli/`) — handles all orchestration: config parsing, profile resolution, symlinks, file permissions natively. Shells out only for package managers and system services.
3. **Configuration** (`conf/`) — declarative TOML files define what to install per profile.

Binary updates are automatic: on first run, the entry script bootstraps the binary. After that, the Rust binary performs update checks and maintains the version cache. On Windows, the PowerShell wrapper also promotes any staged update before relaunching the binary.

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

Platform categories (`linux`, `windows`, `arch`) are auto-detected based on the running OS.

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Configuration is defined in `conf/*.toml` files using TOML format:

- **`profiles.toml`** - Profile definitions
- **`manifest.toml`** - File-to-category mappings for sparse checkout
- **`symlinks.toml`** - Files to symlink to `$HOME`
- **`packages.toml`** - System packages to install
- **`systemd-units.toml`** - Systemd units to enable
- **`vscode-extensions.toml`** - VS Code extensions
- **`copilot-skills.toml`** - GitHub Copilot CLI skills
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
