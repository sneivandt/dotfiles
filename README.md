# Dotfiles ✨

Opinionated, scriptable, cross‑platform (Linux / Arch / Windows) dotfiles with:

- Unified symlinks directory with git sparse checkout filtering
- Profile-based configuration (base, arch, arch-desktop, windows)
- Declarative symlink and package definitions
- Automatic installation of all profile components
- Reproducible test mode + Docker image
- Editor (VS Code) & shell (zsh/bash) configuration

[![Publish Docker image](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)

## Quick Start 🚀

Install with profile selection:
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I --profile arch-desktop
```

Install with interactive profile selection (first time):
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I
# You'll be prompted to select a profile interactively
# Your selection is saved for future runs
```

Re-run with persisted profile (no prompt needed):
```bash
./dotfiles.sh -I
# Uses the previously selected profile automatically
```

Install with verbose logging:
```bash
./dotfiles.sh -I --profile arch-desktop -v
```

Preview changes without modifying the system (dry run):
```bash
./dotfiles.sh -I --profile arch-desktop --dry-run
```

Uninstall (remove managed symlinks):
```bash
./dotfiles.sh -U
```

Dry run uninstall:
```bash
./dotfiles.sh -U --dry-run
```

Help:
```bash
./dotfiles.sh -h
```

## Requirements ⚙️

### Core Requirements

- **Git 2.25+** (January 2020) - for sparse checkout support
- **POSIX shell** (sh, bash, or zsh) - for Linux/Unix systems
- **PowerShell 7+** (PowerShell Core) - for Windows systems (requires Administrator privileges)

### Optional/Feature-Specific Requirements (Linux)

These tools are optional and enable specific features when available:

- **pacman** + **sudo** - for Arch Linux package installation (required for `arch` and `arch-desktop` profiles)
- **systemctl** - for systemd user unit management
- **zsh** - for default shell configuration
- **fc-list** + **fc-cache** - for font configuration
- **code** / **code-insiders** - for VS Code extension installation
- **pwsh** (PowerShell Core) - for PowerShell module management on Linux

The installation script gracefully skips features when required tools are unavailable.

## Usage Summary 📝

```
Usage:
  dotfiles.sh
  dotfiles.sh {-I | --install}   [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]
  dotfiles.sh {-U | --uninstall} [--profile PROFILE] [-v] [--dry-run] [--skip-os-detection]
  dotfiles.sh {-T | --test}      [-v]
  dotfiles.sh {-h | --help}

Options:
  --profile PROFILE     Use predefined profile for sparse checkout
                        Available: base, arch, arch-desktop, windows
                        If not specified:
                          1. Uses previously persisted profile (if exists)
                          2. Prompts interactively to select a profile
                        Selected profile is persisted for future runs.
  -v                    Enable verbose logging
  --dry-run             Perform a dry run without making system modifications.
                        Logs all actions that would be taken. Verbose logging
                        is automatically enabled in dry-run mode for detailed
                        output.
  --skip-os-detection   Skip automatic OS detection overrides. Allows testing
                        arch profile on non-Arch systems. Primarily for CI
                        testing to ensure profile differentiation.
```

**Profile Persistence**: The selected profile is automatically saved to git config
(`.git/config` under `dotfiles.profile`). On subsequent runs without `--profile`,
the script uses the saved profile, making re-runs seamless.

## Profiles 🎯

Profiles define which files are included through git sparse checkout. This allows a single repository to serve multiple environments without checking out unnecessary files.

**Profile Selection**:
- Specify explicitly: `./dotfiles.sh -I --profile arch-desktop`
- Interactive prompt: `./dotfiles.sh -I` (first time or if no profile saved)
- Automatic reuse: `./dotfiles.sh -I` (uses saved profile from previous run)

| Profile | Description | Includes |
|---------|-------------|----------|
| `base` | Minimal setup | Core shell configs only (no OS-specific or desktop files) |
| `arch` | Arch Linux headless | Core shell + Arch packages (no desktop) |
| `arch-desktop` | Arch Linux desktop | Core shell + desktop tools + Arch packages + desktop environment |
| `windows` | Windows | PowerShell + Windows registry + desktop tools (VS Code, IntelliJ IDEA) |

Profiles are defined in [`conf/profiles.ini`](conf/profiles.ini) and map to file categories in [`conf/manifest.ini`](conf/manifest.ini).

### How Profiles Work

1. **Profile Selection**: Choose explicitly via `--profile`, or let the script use the persisted profile from a previous run, or select interactively if none exists
2. **Sparse Checkout**: Git's sparse checkout feature excludes files based on your selected profile
3. **OS Detection** (Linux only): On Linux systems, Arch Linux is auto-detected and non-Arch files are excluded if not running on Arch; Windows files are always excluded on Linux
4. **Auto-Compatibility**: The system applies overrides to ensure compatibility—non-Arch systems exclude Arch-specific files, and Linux systems exclude Windows-specific files, regardless of profile selection
5. **Persistence**: Your profile choice is saved in `.git/config` for seamless re-runs

Example - first time setup with interactive selection:
```bash
./dotfiles.sh -I
# Prompts you to select from available profiles
# Selection is saved automatically
```

Example - switching profiles:
```bash
./dotfiles.sh -I --profile arch
# Desktop files are automatically removed from workspace
# Headless configs remain
# New profile is saved for future runs
```

### Key Files in `conf/`

| File | Description |
|------|-------------|
| `symlinks.ini` | Declarative list of files to link from `symlinks/` to `$HOME`, organized by profile sections (includes `[windows]` section) |
| `packages.ini` | Package list organized by profile sections (e.g., `[arch]`, `[arch,desktop]`) |
| `units.ini` | Systemd user units to enable, organized by profile sections |
| `chmod.ini` | Post-install permission adjustments, organized by profile sections |
| `fonts.ini` | Font families to check/install (single `[fonts]` section, not profile-filtered) |
| `submodules.ini` | Git submodules to initialize |
| `vscode-extensions.ini` | VS Code extensions to install |
| `registry.ini` | **Windows-only**: Registry paths as sections with `key = value` format (no profile filtering) |
| `manifest.ini` | Maps files to categories for sparse checkout exclusion |
| `profiles.ini` | Profile definitions (category include/exclude) |

Most `.ini` files use standard INI format with `[section]` headers containing simple lists.
**Exception**: `registry.ini` uses registry paths as sections with `key = value` pairs (Windows-only).
Profile sections determine which items are processed based on the selected profile. Symlink targets
are always relative to `$HOME` and prefixed with a dot (e.g., `bashrc` → `~/.bashrc`).

## Vim/Neovim Configuration 📝

### Plugin Management

The repository supports **two plugin management approaches**:

1. **Git Submodules (Default)** - Traditional Vim 8+ native pack system
   - Compatible with both Vim and Neovim
   - Plugins managed as git submodules in `.gitmodules`
   - No external dependencies required

2. **lazy.nvim (Modern, Optional)** - Modern Neovim plugin manager
   - Neovim only (Vim 8+ uses submodules)
   - Lazy loading, lockfiles, automatic installation
   - Better dependency management and faster startup
   - Enable with: `export NVIM_USE_LAZY=1`

**Recommendation**: Neovim users should consider enabling lazy.nvim for improved performance and modern plugin management. See [`symlinks/config/nvim/README.md`](symlinks/config/nvim/README.md) for migration details.

Both approaches use the same plugins and configuration, ensuring a consistent experience regardless of the management method chosen.

## Scripts (`./dotfiles.sh`) 🔧

Primary entrypoint: `dotfiles.sh`

Supporting shell utilities reside in `src/linux/`:
* `commands.sh` - High-level install/uninstall/test orchestration
* `tasks.sh` - Granular, idempotent task primitives
* `utils.sh` - Helper predicates + sparse checkout configuration + INI parsing
* `logger.sh` - Logging abstraction

PowerShell modules for Windows reside in `src/windows/`.

### Implementation Highlights

- **Sparse Checkout**: Files excluded by your profile are automatically removed from the working directory
- **Idempotency**: Re-running install only performs missing work
- **Dependency Resolution**: Profile dependencies (e.g., desktop requires certain base files) handled automatically
- **No Backups**: Existing files are removed before linking (by design - commit first!)

### Windows

Windows supports profile-based configuration. The default profile is `windows`.

Usage pattern (PowerShell, elevated as required):
```powershell
.\dotfiles.ps1
# Dry run mode (preview changes without modification)
.\dotfiles.ps1 -DryRun
```

Key differences from Linux:
* Uses PowerShell instead of shell scripts
* Registry settings in addition to symlinks
* Configuration files:
  - `conf/symlinks.ini` - Shared with Linux, Windows uses `[windows]` section
  - `conf/registry.ini` - Registry paths as sections (Windows-only, no profile filtering)

See [docs/WINDOWS.md](docs/WINDOWS.md) for detailed Windows-specific documentation.

## Testing & CI 🧪

This repository includes comprehensive CI testing that validates:

* **Static analysis**: shellcheck (shell scripts) + PSScriptAnalyzer (PowerShell)
* **Configuration validation**: INI file syntax and structure
* **Profile installations**: Dry-run tests for all profiles (base, arch, arch-desktop, windows)
* **Cross-platform**: Tests on both Linux (Ubuntu) and Windows runners
* **Docker build**: Ensures the container image builds successfully

Run tests locally:
```bash
# Run all static analysis and validation
./dotfiles.sh --test -v

# Test installation without making changes (dry-run mode)
./dotfiles.sh --install --profile arch-desktop --dry-run

# Test uninstallation without making changes
./dotfiles.sh --uninstall --dry-run
```

The CI workflow ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) automatically runs on pull requests to validate the project across all supported profiles and platforms. A separate workflow ([`.github/workflows/docker-image.yml`](.github/workflows/docker-image.yml)) builds and publishes the Docker image on pushes to the master branch.

For more detailed information about testing, see [docs/TESTING.md](docs/TESTING.md).

## Docker 🐳

Run the [published image](https://hub.docker.com/r/sneivandt/dotfiles) for an isolated test shell:
```bash
docker run --rm -it sneivandt/dotfiles
```

Build and run locally:
```bash
docker buildx build -t dotfiles:local .
docker run --rm -it dotfiles:local
```

The published image ([`sneivandt/dotfiles`](https://hub.docker.com/r/sneivandt/dotfiles)) is built and pushed by GitHub Actions on pushes to master ([`docker-image.yml`](.github/workflows/docker-image.yml)).

## Customization 🎨

### Adding New Files

1. Add file to `symlinks/` directory
2. Add entry to `conf/symlinks.ini` under appropriate section (e.g., `[base]`, `[arch,desktop]`)
3. (Optional) Add file path to `conf/manifest.ini` if it should be excluded in certain profiles
4. Test with `./dotfiles.sh -I --profile <your-profile>`

### Adding Packages

1. Add package name to `conf/packages.ini` under appropriate section:
   ```ini
   [arch]
   packagename

   [arch,desktop]
   desktop-package
   ```
2. Packages are automatically installed when you use the matching profile

### Creating a Custom Profile

1. Edit `conf/profiles.ini` and add your profile section:
   ```ini
   [my-custom]
   include=
   exclude=windows,desktop
   ```
2. Use with `--profile my-custom`

### Adding a New Category

1. Add category section to `conf/manifest.ini`
2. List all file paths that belong to that category
3. Update profile definitions in `conf/profiles.ini` to include/exclude it

## Troubleshooting 🔍

| Symptom | Check |
|---------|-------|
| Symlink not created | Is source file excluded by sparse checkout? Check `git sparse-checkout list` |
| Package not installed | Correct `conf/packages.ini` section present? Not excluded by profile? Package manager available? |
| Systemd unit inactive | Unit defined in `conf/units.ini` for your profile? Verify with `systemctl --user status <unit>` |
| Sparse checkout not working | Ensure you're in a git repository: `git sparse-checkout list` |
| Wrong files checked out | Verify profile with `echo $PROFILE` and check `conf/profiles.ini` |
| Desktop files missing | Use `--profile arch-desktop` |

## Contributing 🤝

Contributions are welcome! Please see [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) for guidelines on how to contribute to this project.

## Additional Documentation 📚

- [docs/TESTING.md](docs/TESTING.md) - Detailed testing and CI documentation
- [docs/WINDOWS.md](docs/WINDOWS.md) - Windows-specific documentation
- [docs/SECURITY.md](docs/SECURITY.md) - Security policy
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) - Configuration file reference
