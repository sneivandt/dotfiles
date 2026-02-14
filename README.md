# Dotfiles ✨

Cross-platform dotfiles management system with profile-based configuration for Linux, Arch Linux, and Windows.

**Key Features:**
- 🎯 Profile-based configuration (base, arch, arch-desktop, desktop, windows)
- 🔗 Git sparse checkout for environment-specific files
- 📦 Declarative package and symlink management
- 🔄 Idempotent installation (safe to re-run)
- 🤖 GitHub Copilot Agent Skills for development guidance
- 🧪 Comprehensive testing and CI
- 🐳 Docker image for isolated testing

[![Publish Docker image](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)

## Quick Start

### Linux
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I  # Prompts for profile selection
```

### Windows
```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
.\dotfiles.ps1
```

### Docker
```bash
docker run --rm -it sneivandt/dotfiles
```

## Requirements

### Linux
- **Git 2.25+** (for sparse checkout support)
- **POSIX shell** (sh, bash, or zsh)
- **sudo** (for package installation on Arch Linux)

### Windows
- **PowerShell 7+** or **Windows PowerShell 5.1+**
- **Administrator privileges** (for registry and symlink operations)

Optional tools enable additional features:
- `pacman` - Arch Linux package installation
- `systemctl` - Systemd unit management
- `code` - VS Code extension installation
- `gh` - GitHub Copilot CLI skill installation
- `pwsh` - PowerShell module management (Linux)

See [Usage Guide](docs/USAGE.md) for details.

## Usage

```bash
# Install with profile
./dotfiles.sh -I --profile arch-desktop

# Preview changes (dry-run)
./dotfiles.sh -I --dry-run

# Verbose output
./dotfiles.sh -I -v

# Uninstall (remove symlinks)
./dotfiles.sh -U

# Run tests
./dotfiles.sh -T

# Help
./dotfiles.sh -h
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
| `windows` | Windows system (automatic) |

**How it works:**
1. Select a profile (explicitly, interactively, or use saved preference)
2. Git sparse checkout excludes files not needed for your environment
3. Only profile-matching configuration sections are processed
4. Profile selection is saved for future runs

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
- **`fonts.ini`** - Font families to install
- **`chmod.ini`** - File permissions

See the [Configuration Reference](docs/CONFIGURATION.md) for detailed format documentation.

## GitHub Copilot Agent Skills

This repository includes Agent Skills in `.github/skills/` to help GitHub Copilot provide better coding assistance:

**Technical Patterns**:
- **`shell-patterns`** - Shell scripting conventions
- **`powershell-patterns`** - PowerShell scripting conventions
- **`ini-configuration`** - INI file format and parsing
- **`logging-patterns`** - Logging conventions

**System Understanding**:
- **`profile-system`** - Profile-based configuration
- **`symlink-management`** - Symlink management conventions
- **`package-management`** - Package installation patterns

**Development Support**:
- **`customization-guide`** - Adding configuration items programmatically
- **`testing-patterns`** - Testing and validation patterns
- **`git-hooks-patterns`** - Git hooks and security scanning
- **`creating-skills`** - Creating new agent skills

These skills are automatically discovered by GitHub Copilot and help ensure code contributions follow project conventions.

For information on how documentation is organized (instructions vs skills vs docs), see [Documentation Structure](docs/DOCUMENTATION.md).

## Testing

Run all tests (static analysis and configuration validation):
```bash
./dotfiles.sh -T
```

The CI workflow automatically validates:
- Shellcheck and PSScriptAnalyzer (static analysis)
- Configuration file syntax
- Profile installations (dry-run) for all profiles
- Cross-platform compatibility

See [Testing Documentation](docs/TESTING.md) for details.

## Docker

Run in an isolated container:
```bash
docker run --rm -it sneivandt/dotfiles
```

Build locally:
```bash
docker buildx build -t dotfiles:local .
docker run --rm -it dotfiles:local
```

Published image: [`sneivandt/dotfiles`](https://hub.docker.com/r/sneivandt/dotfiles)

See [Docker Documentation](docs/DOCKER.md) for advanced usage.

## Customization

### Add a File
1. Place file in `symlinks/` directory
2. Add entry to `conf/symlinks.ini` under appropriate section
3. Run `./dotfiles.sh -I`

### Add a Package
1. Add package name to `conf/packages.ini` under appropriate section
2. Run `./dotfiles.sh -I`

### Create a Profile
1. Define in `conf/profiles.ini`
2. Use with `--profile <your-profile>`

See the [Customization Guide](docs/CUSTOMIZATION.md) for detailed instructions.

## Documentation

- **[Usage Guide](docs/USAGE.md)** - Detailed installation and usage instructions
- **[Profile System](docs/PROFILES.md)** - Understanding and using profiles
- **[Configuration Reference](docs/CONFIGURATION.md)** - Configuration file formats
- **[Customization Guide](docs/CUSTOMIZATION.md)** - Adding files, packages, and profiles
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[Windows Usage](docs/WINDOWS.md)** - Windows-specific documentation
- **[Docker](docs/DOCKER.md)** - Docker image usage and building
- **[Architecture](docs/ARCHITECTURE.md)** - Implementation and design details
- **[Testing](docs/TESTING.md)** - Testing procedures and CI
- **[Contributing](docs/CONTRIBUTING.md)** - Contribution guidelines
- **[Git Hooks](docs/HOOKS.md)** - Repository git hooks
- **[Security](docs/SECURITY.md)** - Security policy and best practices

See [docs/README.md](docs/README.md) for a complete documentation index.
