# GitHub Copilot Project Instructions

These guidelines help AI code assistants produce consistent, safe contributions to this dotfiles project.

> **Note**: This project uses GitHub Copilot Agent Skills for detailed technical guidance. See `.github/skills/` for available skills.

## Project Overview
This project manages dotfiles and system configuration using a profile-based sparse checkout approach. It supports both Linux (specifically Arch Linux) and Windows.
- **Profile-Based Configuration**: Uses profiles defined in `conf/profiles.ini` that control which files are checked out via git sparse checkout. See the `profile-system` skill for details.
- **INI Configuration Format**: All configuration files (`conf/*.ini`) use standard INI format. See the `ini-configuration` skill for details.
- **Idempotency**: All scripts are designed to be idempotent. Re-running the installation should simply verify the state without side effects or errors.
- **Cross-Platform**: The project uses `dotfiles.sh` (POSIX sh) for Linux and `dotfiles.ps1` (PowerShell) for Windows.
- **Automatic Installation**: Profile components (packages, units, symlinks, fonts) are automatically installed based on configuration - no flags needed.
- **Goals**:
  - Provide reproducible, profile-based environment setup.
  - Keep scripts POSIX `/bin/sh` compatible.
  - Favor clarity over brevity; explicit checks and logging are preferred.

## Repository Structure
Key directories and their purposes:

### `conf/` - Configuration Files (INI Format)
All configuration files use standard INI format with section headers. See the `ini-configuration` skill for complete details.
- **`profiles.ini`**: Profile definitions with include/exclude categories
- **`manifest.ini`**: Maps files to categories for sparse checkout exclusion
- **`symlinks.ini`**: Symlink mappings organized by category sections (e.g., `[base]`, `[arch,desktop]`)
- **`packages.ini`**: System packages organized by category sections
- **`units.ini`**: Systemd user units organized by category sections
- **`chmod.ini`**: File permissions organized by category sections
- **`fonts.ini`**: Font families to check/install
- **`vscode-extensions.ini`**: VS Code extensions in `[extensions]` section
- **`registry.ini`**: Windows registry settings with registry paths as sections
- **`copilot-skills.ini`**: External GitHub Copilot CLI skill URLs to download

### `symlinks/` - Linkable Files
The source directory for all dotfiles to be symlinked. Files here are filtered by git sparse checkout based on the selected profile. Structure mirrors the target layout under `$HOME` (with dots prepended).

### `src/linux/` - Linux Shell Scripts
- **`commands.sh`**: High-level orchestration (do_install, do_uninstall, do_test)
- **`tasks.sh`**: Granular, idempotent task primitives
- **`utils.sh`**: Helper predicates, sparse checkout logic, INI parsing
- **`logger.sh`**: Logging utilities
- **`script.psm1`**: PowerShell module management (used when pwsh is available on Linux)

### `src/windows/` - Windows PowerShell Modules
- **`Profile.psm1`**: Profile filtering and INI parsing utilities
- **`Symlinks.psm1`**: Windows symlink installation
- **`Registry.psm1`**: Registry configuration
- **`VsCodeExtensions.psm1`**: VS Code extension installation

## Profile System
See the `profile-system` skill for complete details on:
- How profiles work (sparse checkout, section filtering, persistence)
- Available profiles (base, arch, arch-desktop, desktop, windows)
- Auto-detection overrides
- Profile selection priority and usage
- Adding new profiles and configuration items

## INI File Format
See the `ini-configuration` skill for complete details on:
- Section-based configuration format
- Section naming conventions (hyphens vs comma-separated)
- Special case: registry configuration
- Parsing INI files (shell and PowerShell)
- Profile filtering helpers

## Symlink Management
Symlinks are managed declaratively through `conf/symlinks.ini`.
- **Configuration**: `conf/symlinks.ini` uses INI sections for each profile (e.g., `[base]`, `[arch,desktop]`, `[windows]`)
- **Source**: Source files are located in `symlinks/<path>` at the repository root (without leading dot)
- **Target** (Linux): Targets are relative to `$HOME`, prefixed with a dot by the script
  - Example: `config/nvim` in `[base]` section maps `symlinks/config/nvim` to `~/.config/nvim`
- **Target** (Windows): Targets are relative to `%USERPROFILE%`, with smart dot-prefixing:
  - Well-known Windows folders (AppData, Documents, etc.) are NOT prefixed with a dot
  - Unix-style paths (config, ssh, etc.) ARE prefixed with a dot
  - Example: `AppData/Roaming/Code/User/settings.json` → `%USERPROFILE%\AppData\Roaming\Code\User\settings.json`
  - Example: `config/git/config` → `%USERPROFILE%\.config\git\config`
- **Rule**: Do not hardcode `ln -s` commands. Always add files to `symlinks/` and add entries (without leading dot) to appropriate sections in `conf/symlinks.ini`
- **Backups**: Do not backup existing files before linking. Files are removed and replaced by symlinks.

## Shell Scripting
See the `shell-patterns` skill for complete details on:
- Script headers and POSIX compatibility
- Code style (conditionals, quoting, etc.)
- Logging helpers (log_stage, log_verbose, log_error, log_dry_run)
- Task function pattern (subshell isolation)
- Idempotency and dry-run patterns
- Helper predicates (is_program_installed, is_dry_run, should_include_profile_tag)
- Package management patterns

## PowerShell
See the `powershell-patterns` skill for complete details on:
- Code style (Verb-Noun functions, comment-based help)
- Logging conventions (stage headers, dry-run, verbose)
- Idempotency and dry-run patterns
- INI parsing and profile filtering helpers
- Configuration processing patterns
- Error suppression guidelines

## File Formatting
- **No Trailing Whitespace**: Never leave trailing whitespace at the end of lines in any file
  - This applies to all file types: shell scripts, PowerShell, INI files, Markdown, configuration files
  - Trailing whitespace causes unnecessary git diffs and is considered poor coding hygiene
  - Most editors can be configured to automatically remove trailing whitespace on save
  - When creating or editing files, always ensure lines end cleanly without trailing spaces or tabs

## Testing & CI
- Perform static analysis by running `dotfiles.sh -T` (or `--test`)
  - Runs `test_config_validation` - validates INI file syntax and structure
  - Runs `test_shellcheck` - shell script linting for all `.sh` files
  - Runs `test_psscriptanalyzer` - PowerShell script analysis for all `.ps1`/`.psm1` files
- Ensure all scripts are idempotent; re-running should not cause errors or unintended changes.
- Test with different profiles to ensure sparse checkout works correctly.
- Use verbose mode (`-v`) for debugging: `./dotfiles.sh -I --profile arch-desktop -v`
- Use dry-run mode to preview changes: `./dotfiles.sh -I --dry-run` (auto-enables verbose)
- CI workflow (`.github/workflows/ci.yml`) runs automatically on pull requests to validate:
  - Static analysis (shellcheck and PSScriptAnalyzer)
  - Configuration file validation
  - Profile installations with dry-run tests (base, arch, arch-desktop, desktop, windows)
  - Cross-platform compatibility (Linux Ubuntu and Windows runners)
  - Docker image build
- Docker image workflow (`.github/workflows/docker-image.yml`) publishes to Docker Hub on pushes to master branch
