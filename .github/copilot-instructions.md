# GitHub Copilot Project Instructions

These guidelines help AI code assistants produce consistent, safe contributions to this dotfiles project.

## Project Overview
This project manages dotfiles and system configuration using a profile-based sparse checkout approach. It supports both Linux (specifically Arch Linux) and Windows.
- **Profile-Based Configuration**: Configuration uses profiles (e.g., `base`, `arch`, `arch-desktop`, `windows`) defined in `conf/profiles.ini` that control which files are checked out via git sparse checkout.
- **INI Configuration Format**: All configuration files (`conf/*.ini`) use standard INI format with `[section]` headers for organization.
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
All configuration files use standard INI format with section headers:
- **`profiles.ini`**: Profile definitions with include/exclude categories
- **`manifest.ini`**: Maps files to categories for sparse checkout exclusion
- **`symlinks.ini`**: Symlink mappings organized by category sections (e.g., `[base]`, `[arch,desktop]`)
- **`packages.ini`**: System packages organized by category sections
- **`units.ini`**: Systemd user units organized by category sections
- **`chmod.ini`**: File permissions organized by category sections
- **`fonts.ini`**: Font families to check/install
- **`submodules.ini`**: Git submodules organized by category
- **`vscode-extensions.ini`**: VS Code extensions in `[extensions]` section
- **`registry.ini`**: Windows registry settings with registry paths as sections

**Important**: Profile names (like `arch-desktop` in profiles.ini) use hyphens. Section names in other config files use comma-separated categories (like `[arch,desktop]`) to indicate ALL listed categories must be active.

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
- **`Font.psm1`**: Font installation
- **`Git.psm1`**: Git submodule management
- **`VsCodeExtensions.psm1`**: VS Code extension installation

### `extern/` - External Dependencies
Git submodules (e.g., powerline fonts) that are initialized based on configuration.

## Profile System
Profiles control which files are checked out and which configuration sections are processed. This allows a single repository to support multiple environments (Linux headless, Linux desktop, Windows) while only checking out relevant files.

### How Profiles Work
1. **Sparse Checkout**: Git sparse checkout excludes files based on categories defined in `conf/manifest.ini`. This reduces disk usage and clutter by only checking out files relevant to the selected profile.
2. **Section Filtering**: Configuration files use section headers that match profiles to determine which items to process
3. **Automatic Installation**: All components defined in active profile sections are automatically installed
4. **Persistence**: Selected profile is saved in `.git/config` for automatic reuse on subsequent runs

### Auto-Detection Overrides
**IMPORTANT**: System detection always takes precedence over profile configuration to prevent incompatible operations:
- **Non-Arch Linux systems**: Always exclude `arch` category regardless of selected profile
- **Linux systems**: Always exclude `windows` category regardless of selected profile

These overrides ensure compatibility even if an incompatible profile is manually selected.

### Profile Selection Priority
When running `dotfiles.sh`, the profile is determined in this order:
1. **Explicit CLI argument**: `--profile arch-desktop` (highest priority)
2. **Persisted profile**: Reads from `.git/config` (`dotfiles.profile` key)
3. **Interactive prompt**: If neither exists, prompts user to select from available profiles

Example usage:
```bash
# First time - interactive selection
./dotfiles.sh -I
# Prompts: "Select profile (1-4): "
# Selection is saved to .git/config

# Subsequent runs - uses saved profile
./dotfiles.sh -I
# No prompt, uses persisted profile

# Override saved profile
./dotfiles.sh -I --profile base
# Uses 'base' and updates saved profile
```

### Available Profiles
- **`base`**: Minimal core shell configuration (excludes OS-specific and desktop files)
- **`arch`**: Arch Linux headless (includes Arch packages, excludes desktop)
- **`arch-desktop`**: Arch Linux desktop (includes desktop tools, window manager, fonts)
- **`windows`**: Windows environment (PowerShell, registry settings)

### Profile Persistence Implementation
Profile persistence uses git config:
- **Save**: `git config --local dotfiles.profile <profile_name>`
- **Read**: `git config --local --get dotfiles.profile`
- **Location**: `.git/config` (not committed, local to repository clone)

See `src/linux/utils.sh` for implementation:
- `get_persisted_profile()`: Reads saved profile
- `persist_profile()`: Saves profile
- `prompt_profile_selection()`: Interactive selection UI
- `list_available_profiles()`: Reads from `conf/profiles.ini`

### Adding New Profiles
1. Add profile definition to `conf/profiles.ini`:
   ```ini
   [my-profile]
   include=
   exclude=windows,desktop
   ```
2. Use with `--profile my-profile` or select interactively

### Adding Configuration Items
When adding packages, units, or other configuration:
1. Add to appropriate INI file under the correct section using comma-separated categories
   - Single category: `[arch]` for Arch-only items
   - Multiple categories: `[arch,desktop]` for items requiring BOTH Arch AND desktop
2. Item will be automatically processed when ALL required categories are active
3. No flags needed - profile determines what gets installed

## INI File Format
All configuration files in `conf/` use standard INI format:

### Section-Based Configuration
Most INI files use simple list format (one item per line):
```ini
# Comments start with #
[section-name]
entry-one
entry-two

[another-section]
more-entries
```

**Important Distinction**:
- **Profile names** (in `profiles.ini` only): Use hyphens like `[arch-desktop]`
- **Section names** (all other .ini files): Use comma-separated categories like `[arch,desktop]`
  - Comma-separated sections mean ALL categories must be active (logical AND)
  - Example: `[arch,desktop]` is processed only when both `arch` and `desktop` are not excluded

### Special Case: Windows Registry Configuration
**Exception**: `conf/registry.ini` is the ONLY config file using `key = value` format:
```ini
[HKCU:\Software\Example]
SettingName = SettingValue
AnotherSetting = AnotherValue
```
Section headers are registry paths, and assignments are registry key/value pairs. All other INI files use simple lists.

### Parsing Rules
- Use `read_ini_section()` helper from `src/linux/utils.sh` to read sections
- Empty lines and `#` comments are ignored
- Section names match profile names or categories
- Process only sections that match the active profile using `should_include_profile_tag()`

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
- Use `#!/bin/sh` unless there is a compelling reason for Bash. If Bash required, document it.
- Always start new shell scripts with:
  ```sh
  #!/bin/sh
  set -o errexit
  set -o nounset
  ```
- Use double quotes around variable expansions except when intentional word splitting is required (add a shellcheck directive comment there).
- Avoid process substitution and arrays (Bash features) in POSIX scripts.
- **Conditional statements**: Always use compact style with `then` on the same line:
  ```sh
  if [ condition ]; then
    # code
  fi
  ```
  NOT:
  ```sh
  if [ condition ]
  then
    # code
  fi
  ```
- **Logging**: Use existing helpers instead of ad-hoc echo statements
  - `log_stage "Stage Name"` - Stage headers (prints once per stage with `::` prefix)
    - Uses `_work` flag to print only once per subshell, even if called multiple times
    - Resets automatically in new subshell (see Task Function Pattern below)
  - `log_verbose "Message"` - Verbose details (only shown with `-v` flag)
  - `log_error "Error"` - Error messages (exits with status 1)
  - `log_dry_run "Would <action>"` - Dry-run actions (always shown in dry-run mode)
- **Guard Optional Tools**: Use `is_program_installed` predicate before using external tools
  - Example: `if ! is_program_installed "tool"; then log_verbose "Skipping: tool not installed"; return; fi`
- **INI Parsing**: Use `read_ini_section()` helper from `utils.sh` to parse INI configuration files
  - Reads a specific section and outputs one line per entry (pipe to `while read` loop)
  - Example: `read_ini_section "$DIR/conf/file.ini" "section" | while IFS='' read -r item`
- **Profile Filtering**: Use `should_include_profile_tag()` to check if a section/profile should be processed
  - Returns 0 (success) if ALL required categories in tag are NOT excluded
  - Logic: Exclude section if ANY required category is in EXCLUDED_CATEGORIES
  - Example: `if should_include_profile_tag "$section"; then process_section; fi`
- **Task Function Pattern**: Always wrap task functions in subshell `( )` for environment isolation
  - Syntax: `my_task() {( ... )}` puts function body in subshell
  - Benefits: Isolates variables, directory changes, and `_work` flag state
  - Each subshell gets fresh `_work` flag, so `log_stage` prints once per task
- **Idempotency**: Always check if action is needed before taking it
  - Check file existence, symlink targets, package installation status
  - Skip with verbose log if already correct: `log_verbose "Skipping: already correct"`
- **Dry-Run Pattern**: Check `is_dry_run` before system modifications
  ```sh
  if is_dry_run; then
    log_dry_run "Would perform action"
  else
    log_verbose "Performing action"
    # actual work
  fi
  ```
- Prefer constructing minimal lists before calling system package managers.
- Always quote glob patterns when iterating variable-expanded paths.

## PowerShell
- Match existing style: Verb-Noun function names, comment-based help, export only necessary functions via `Export-ModuleMember`.
- Windows automation should fail gracefully when run without elevation if elevation is required.
- Use `Test-ShouldIncludeSection` from `Profile.psm1` to filter sections when `$excludedCategories` is available
- Use `Get-ProfileExclusion` to resolve profile to excluded categories in main script
- Use `Read-IniSection` to read configuration sections
- **Configuration Format**:
  - `conf/symlinks.ini`: Uses `[windows]` section with paths relative to `symlinks/` (no leading dot)
    - Well-known Windows folders (AppData, Documents, etc.) remain as-is in target
    - Unix-style paths (config, ssh) get prefixed with dot by Symlinks.psm1
  - `conf/registry.ini`: Registry paths as sections with `name = value` format
    - No profile filtering (Windows-only by nature)
  - All other INI files follow section-based format like Linux (e.g., `[windows]`, `[base]`)
- **INI Parsing**: Always use `Read-IniSection` helper from `Profile.psm1` instead of manual parsing
  - Reads a specific section from an INI file
  - Returns array of non-empty, non-comment lines
  - Example: `$fonts = Read-IniSection -FilePath $configFile -SectionName "fonts"`
- **Section Filtering**: Use `Test-ShouldIncludeSection` to check if a section should be processed
  - Returns `$true` if ALL required categories in section name are NOT excluded
  - Example: `if (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories)`
- **Logging Conventions**:
  - Stage headers: `Write-Output ":: Stage Name"` (with `::` prefix, only once per stage using `$act` flag)
  - Dry-run actions: `Write-Output "DRY-RUN: Would <action>"`
  - Verbose details: `Write-Verbose "<message>"` for routine operations
  - Skipping actions: `Write-Verbose "Skipping <item>: <reason>"`
  - Use `$act` flag to print stage header only once when first action is taken
- **Idempotency**: Check if action is needed before taking it
  - Check file existence, registry values, installed packages/extensions
  - Skip with verbose message if already correct: `Write-Verbose "Skipping <item>: already <state>"`
- **Dry-Run Pattern**: All functions support `-DryRun` switch
  - Check `if ($DryRun)` before any system modification
  - Log intended action with `Write-Output "DRY-RUN: Would <action>"`
  - Never modify system state when `$DryRun` is set
- **Common Patterns**:
  - Reading from config: `$items = Read-IniSection -FilePath $configFile -SectionName "section"`
  - Checking sections: `if (-not (Test-ShouldIncludeSection ...)) { Write-Verbose "Skipping..."; continue }`
  - Stage logging: `if (-not $act) { $act = $true; Write-Output ":: Stage" }`
  - Error suppression: Use `-ErrorAction SilentlyContinue` only when appropriate, prefer explicit checks

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
  - Profile installations with dry-run tests (base, arch, arch-desktop, windows)
  - Cross-platform compatibility (Linux Ubuntu and Windows runners)
  - Docker image build
- Docker image workflow (`.github/workflows/docker-image.yml`) publishes to Docker Hub on pushes to master branch

## Common Patterns

### Reading INI Sections
```sh
read_ini_section "$DIR/conf/packages.ini" "arch" | while IFS='' read -r package
do
  # Process package
done
```

### Checking Profile Inclusion
```sh
if should_include_profile_tag "$section"; then
  # Process this section
fi
```

### Processing Multi-Section Configs
```sh
# Get all sections from config file
sections="$(grep -E '^\[.+\]$' "$DIR"/conf/file.ini | tr -d '[]')"

# Process each section that matches the profile
for section in $sections; do
  if ! should_include_profile_tag "$section"; then
    log_verbose "Skipping section [$section]: profile not included"
    continue
  fi

  # Process entries in this section
  read_ini_section "$DIR"/conf/file.ini "$section" | while IFS='' read -r item; do
    # Process item
  done
done
```

### Task Function Structure
```sh
my_task()
{(
  # Check prerequisites
  if ! is_program_installed "tool"; then
    log_verbose "Skipping task: tool not installed"
    return
  fi

  # Check if config exists
  if [ ! -f "$DIR"/conf/config.ini ]; then
    log_verbose "Skipping task: no config.ini"
    return
  fi

  # Do work (log_stage prints once per subshell)
  log_stage "Task name"

  # Dry-run pattern
  if is_dry_run; then
    log_dry_run "Would perform action"
  else
    log_verbose "Performing action"
    # actual work
  fi
)}
```
