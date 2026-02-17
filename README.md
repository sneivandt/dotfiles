# Dotfiles ✨

Cross-platform dotfiles management system powered by a Rust core engine, with profile-based configuration for Linux, Arch Linux, and Windows.

**Key Features:**
- 🦀 Rust core engine — fast, reliable, cross-platform binary
- 🎯 Profile-based configuration (base, arch, arch-desktop, desktop, windows)
- 🔗 Git sparse checkout for environment-specific files
- 📦 Declarative package and symlink management via INI config
- 🔄 Idempotent installation (safe to re-run)
- 📡 Automatic binary updates from GitHub Releases
- 🤖 GitHub Copilot Agent Skills for development guidance
- 🧪 Comprehensive Rust test suite and CI
- 🐳 Docker image for isolated testing

[![CI](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/ci.yml)
[![Publish Docker image](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)

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
.\dotfiles.ps1 install -p windows
```

### Docker
```bash
docker run --rm -it sneivandt/dotfiles
```

## How It Works

The dotfiles system has three layers:

1. **Entry scripts** (`dotfiles.sh`, `dotfiles.ps1`) — thin wrappers that download the latest binary from GitHub Releases (or build from source with `--build`) and forward all arguments.
2. **Rust binary** (`cli/`) — handles all orchestration: config parsing, profile resolution, symlinks, file permissions natively. Shells out only for package managers and system services.
3. **Configuration** (`conf/`) — declarative INI files define what to install per profile.

Binary updates are automatic: on first run, the entry script downloads the binary. On subsequent runs, a version cache ensures no delay if the binary is already current.

## Usage

```bash
# Install with profile
./dotfiles.sh install -p arch-desktop

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
| `arch` | Arch Linux headless (server) |
| `arch-desktop` | Arch Linux with desktop environment |
| `desktop` | Generic Linux desktop (non-Arch) |
| `windows` | Windows system |

See the [Profile System Guide](docs/PROFILES.md) for details.

## Configuration

Configuration is defined in `conf/*.ini` files using standard INI format:

- **`profiles.ini`** - Profile definitions
- **`manifest.ini`** - File-to-category mappings for sparse checkout
- **`symlinks.ini`** - Files to symlink to `$HOME`
- **`packages.ini`** - System packages to install
- **`units.ini`** - Systemd units to enable
- **`vscode-extensions.ini`** - VS Code extensions
- **`copilot-skills.ini`** - GitHub Copilot CLI skills
- **`registry.ini`** - Windows registry settings
- **`fonts.ini`** - Font families to check
- **`chmod.ini`** - File permissions

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
- **[Customization Guide](docs/CUSTOMIZATION.md)** - Adding files, packages, and profiles
- **[Architecture](docs/ARCHITECTURE.md)** - Rust engine design and structure
- **[Contributing](docs/CONTRIBUTING.md)** - Development workflow and guidelines
- **[Testing](docs/TESTING.md)** - Testing procedures and CI
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[Windows Usage](docs/WINDOWS.md)** - Windows-specific documentation
- **[Docker](docs/DOCKER.md)** - Docker image usage and building
- **[Git Hooks](docs/HOOKS.md)** - Repository git hooks
- **[Security](docs/SECURITY.md)** - Security policy and best practices
