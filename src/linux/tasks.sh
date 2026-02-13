#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# tasks.sh
# -----------------------------------------------------------------------------
# Collection of granular, mostly idempotent task primitives used by higher
# level orchestration in commands.sh. Each function performs a narrow unit of
# work and self‑guards (state checks) to avoid redundant operations.
#
# Concurrency / Subshells:
#   Each task executes in a subshell `( )` so that temporary variable or
#   directory changes do not leak into the caller environment.
#
# Naming Convention:
#   install_*    Introduce or ensure presence of an artifact.
#   configure_*  Adjust system/user settings post‑installation.
#   update_*     Fetch newer versions of existing artifacts.
#   test_*       Perform validation / static analysis.
#   uninstall_*  Remove managed artifacts.
#
# Utilities / Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (is_program_installed, read_ini_section, should_include_profile_tag)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
#   OPT  CLI options string (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

# configure_file_mode_bits
#
# Apply chmod directives declared in conf/chmod.ini.
# Reads from sections matching current profile.
#
# Implementation Notes:
#   * Uses INI format with [section] headers for each profile.
#   * Each line in section: <mode> <relative-path-under-home>
#   * Uses -R allowing directories to be targeted; user path is prefixed with
#     a dot (".") to match symlink convention.
#   * Skips entries where target file doesn't exist (handles sparse checkout gracefully).
configure_file_mode_bits()
{(
  # Check if chmod.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/chmod.ini ]; then
    log_verbose "Skipping chmod: no chmod.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/chmod.ini; then
    log_verbose "Skipping chmod: no sections match active profile"
    return
  fi

  log_progress "Checking file permissions..."
  log_verbose "Processing chmod config: conf/chmod.ini"

  # Get list of sections from chmod.ini
  # Sections use comma-separated categories (e.g., [base], [arch,desktop])
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/chmod.ini | tr -d '[]')"

  local act=0

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping chmod section [$section]: profile not included"
      continue
    fi

    # Read entries from this section
    read_ini_section "$DIR"/conf/chmod.ini "$section" | while IFS='' read -r line || [ -n "$line" ]; do
      # Skip empty lines
      if [ -z "$line" ]; then
        continue
      fi

      # Parse mode and file from line (format: <mode> <file>)
      # Use awk to handle any whitespace (spaces or tabs) between fields
      mode="$(echo "$line" | awk '{print $1}')"
      file="$(echo "$line" | awk '{$1=""; sub(/^[[:space:]]+/, ""); print}')"

      if [ -z "$mode" ] || [ -z "$file" ]; then
        continue
      fi

      target="$HOME/.$file"

      # Check if the target file exists - skip gracefully if not
      if [ ! -e "$target" ]; then
        log_verbose "Skipping chmod on $target: file does not exist"
        continue
      fi

      # Check if mode is already correct
      # For files/symlinks, check the target's permissions using -L
      current_mode="$(stat -L -c '%a' "$target" 2>/dev/null || echo '')"
      if [ "$current_mode" = "$mode" ]; then
        # Permissions already correct
        log_verbose "Skipping chmod on $target: permissions already correct"
        continue
      fi

      # Apply chmod
      if [ $act -eq 0 ]; then
        act=1
        log_stage "Configuring file permissions"
      fi
      if is_dry_run; then
        log_dry_run "Would set mode $mode on $target"
        increment_counter "chmod_applied"
      else
        log_verbose "Setting mode $mode on $target"
        # Note: -R flag applies mode recursively to ALL files/directories
        chmod -c -R "$mode" "$target"
        increment_counter "chmod_applied"
      fi
    done
  done
)}

# configure_fonts
#
# Refresh font cache when GUI fonts list (conf/fonts.ini) differs from currently
# installed families. Processes only sections matching the active profile.
# Skips if: required fc-* tools missing, fonts.ini missing, no matching profile
# sections, or all listed font families already present.
configure_fonts()
{(
  # Check if font configuration tools are installed
  if ! is_program_installed "fc-list" || ! is_program_installed "fc-cache"; then
    log_verbose "Skipping font configuration: fc-list or fc-cache not installed (install fontconfig)"
    return
  fi

  # Check if fonts.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/fonts.ini ]; then
    log_verbose "Skipping font configuration: no fonts.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/fonts.ini; then
    log_verbose "Skipping font configuration: no sections match current profile"
    return
  fi

  log_progress "Checking fonts..."
  log_verbose "Checking fonts from: conf/fonts.ini"

  # Create temp file with cleanup trap
  local tmpfile
  tmpfile="$(mktemp)"
  trap 'rm -f "$tmpfile"' EXIT

  # Get all sections from fonts.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/fonts.ini | tr -d '[]')"

  # Track if any fonts are missing across all relevant sections
  missing_fonts=0

  for section in $sections; do
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping font section [$section]: profile not included"
      continue
    fi

    # Read fonts from this section
    read_ini_section "$DIR"/conf/fonts.ini "$section" > "$tmpfile"

    while IFS='' read -r font || [ -n "$font" ]; do
      if [ -z "$font" ]; then
        continue
      fi

      # Check if the font family is already installed in the system
      # fc-list outputs comma-separated family names, so we need to handle that
      # Convert commas to newlines and check for exact match of any family name
      if ! fc-list : family | tr ',' '\n' | grep -Fxq "$font"; then
        missing_fonts=1
        break 2  # Break out of both loops
      fi
    done < "$tmpfile"
  done
  rm -f "$tmpfile"

  if [ "$missing_fonts" -eq 0 ]; then
    log_verbose "Skipping font configuration: fonts already up to date"
    return
  fi

  log_stage "Updating fonts"
  if is_dry_run; then
    log_dry_run "Would run fc-cache to update font cache"
    increment_counter "fonts_cache_updated"
  else
    log_verbose "Running fc-cache to update font cache"
    fc-cache
    increment_counter "fonts_cache_updated"
  fi
)}

# configure_shell
#
# Change default login shell to zsh when available and not already set.
# Skip inside Docker (/.dockerenv) and when passwd status indicates locked
# account. Uses chsh invoking absolute path resolved via a nested zsh.
configure_shell()
{(
  if ! is_program_installed "zsh"; then
    log_verbose "Skipping shell configuration: zsh not installed"
    return
  fi

  # Resolve the absolute path to zsh
  zsh_path="$(zsh -c "command -vp zsh")"
  if [ "$SHELL" = "$zsh_path" ]; then
    log_verbose "Skipping shell configuration: shell already set to zsh"
    return
  fi

  # Do not change shell if running inside a Docker container
  if [ -f /.dockerenv ]; then
    log_verbose "Skipping shell configuration: running inside Docker"
    return
  fi

  # Check if the user account is usable (password status is 'P' for usable password)
  if [ "$(passwd --status "$USER" | cut -d" " -f2)" != "P" ]; then
    log_verbose "Skipping shell configuration: user account not usable (passwd status)"
    return
  fi

  log_stage "Configuring user shell"
  if is_dry_run; then
    log_dry_run "Would change shell to $zsh_path"
  else
    log_verbose "Changing shell to $zsh_path"
    chsh -s "$zsh_path"
  fi
)}

# configure_systemd
#
# Enable (and start when user session active) user-level systemd units listed
# in each section of units.ini. Only units already installed (list-unit-files)
# are considered. Avoids starting during early boot by checking
# `systemctl is-system-running` state.
configure_systemd()
{(
  # Check if the system is actually running under systemd (PID 1 is systemd)
  if [ "$(ps -p 1 -o comm=)" != "systemd" ]; then
    log_verbose "Skipping systemd configuration: not running under systemd"
    return
  fi

  if ! is_program_installed "systemctl"; then
    log_verbose "Skipping systemd configuration: systemctl not installed"
    return
  fi

  # Check if units.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/units.ini ]; then
    log_verbose "Skipping systemd units: no units.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/units.ini; then
    log_verbose "Skipping systemd units: no sections match active profile"
    return
  fi

  log_progress "Checking systemd units..."
  log_verbose "Processing systemd units from: conf/units.ini"

  # Get list of sections from units.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/units.ini | tr -d '[]')"

  # Collect all units that need to be enabled across all sections
  # to print stage header only once
  local tmpfile
  tmpfile="$(mktemp)"
  trap 'rm -f "$tmpfile"' EXIT
  for section in $sections
  do
    if should_include_profile_tag "$section"; then
      read_ini_section "$DIR"/conf/units.ini "$section" | while IFS='' read -r unit || [ -n "$unit" ]
      do
        if [ -n "$unit" ] && systemctl --user list-unit-files | cut -d" " -f1 | grep -qx "$unit"; then
          if ! systemctl --user is-enabled --quiet "$unit"; then
            echo "$unit"
          fi
        fi
      done
    fi
  done | sort -u > "$tmpfile"
  units_to_enable="$(cat "$tmpfile")"
  rm -f "$tmpfile"

  if [ -n "$units_to_enable" ]; then
    log_stage "Configuring systemd"
  fi

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping systemd section [$section]: profile not included"
      continue
    fi

    # Read units from this section
    read_ini_section "$DIR"/conf/units.ini "$section" | while IFS='' read -r unit || [ -n "$unit" ]
    do
      if [ -z "$unit" ]; then
        continue
      fi

      # Check if the unit file is known to systemd
      if ! systemctl --user list-unit-files | cut -d" " -f1 | grep -qx "$unit"; then
        log_verbose "Skipping systemd unit $unit: not found in unit files"
      elif systemctl --user is-enabled --quiet "$unit"; then
        log_verbose "Skipping systemd unit $unit: already enabled"
      else
        if is_dry_run; then
          log_dry_run "Would enable systemd unit: $unit"
          if [ "$(systemctl is-system-running)" = "running" ]; then
            log_dry_run "Would start systemd unit: $unit"
          fi
          increment_counter "systemd_units_enabled"
        else
          log_verbose "Enabling systemd unit: $unit"
          systemctl --user enable "$unit"
          increment_counter "systemd_units_enabled"

          # Only start the unit if the system is fully running (avoids issues during early boot)
          if [ "$(systemctl is-system-running)" = "running" ]; then
            log_verbose "Starting systemd unit: $unit"
            # Allow start to fail (e.g., services requiring display server in headless CI)
            if ! systemctl --user start "$unit"; then
              log_verbose "Warning: Failed to start $unit - service may not be available in this environment"
            fi
          fi
        fi
      fi
    done
  done
)}

# _install_package_list
#
# Internal helper to install a list of packages with a specific command.
# Handles dry-run mode, package counting, and counter increments.
#
# Args:
#   $1  package_list - space-separated list of packages to install
#   $2  install_command - command to run (e.g., "sudo pacman -S --quiet --needed --noconfirm" or "paru -S --needed --noconfirm")
#   $3  stage_message - message for log_stage (e.g., "Installing packages" or "Installing AUR packages")
#   $4  counter_name - counter to increment (e.g., "packages_installed" or "aur_packages_installed")
#
# Result:
#   0 success or dry-run, 1 if some packages failed
_install_package_list()
{
  local packages="$1"
  local install_command="$2"
  local stage_message="$3"
  local counter_name="$4"

  if [ -z "$packages" ]; then
    return 0
  fi

  # Count packages in the list
  local package_count
  package_count=$(echo "$packages" | wc -w)

  if is_dry_run; then
    log_stage "$stage_message"
    log_dry_run "Would install packages: $packages"
    # Count packages for dry-run summary
    local count=0
    while [ "$count" -lt "$package_count" ]; do
      increment_counter "$counter_name"
      count=$((count + 1))
    done
  else
    log_verbose "Checking if packages need installation: $packages"
    # shellcheck disable=SC2086  # Word splitting intentional: $packages is space-separated list
    if $install_command $packages 2>&1; then
      log_stage "$stage_message"
      # Count packages that were in the install list (they needed installation)
      local count=0
      while [ "$count" -lt "$package_count" ]; do
        increment_counter "$counter_name"
        count=$((count + 1))
      done
    else
      log_verbose "Warning: Some packages may have failed to install"
    fi
  fi
}

install_aur_packages()
{(
  if ! is_program_installed "paru"; then
    log_verbose "Skipping AUR packages: paru not installed"
    return
  fi

  # Check if packages.ini exists
  if [ ! -f "$DIR"/conf/packages.ini ]; then
    log_verbose "Skipping AUR packages: no packages.ini found"
    return
  fi

  log_progress "Checking AUR packages..."

  packages=""
  log_verbose "Processing AUR packages from: conf/packages.ini"

  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/packages.ini | tr -d '[]')"

  for section in $sections
  do
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping AUR packages section [$section]: profile not included" >&2
      continue
    fi

    # Only process sections containing 'aur' tag
    case ",$section," in
      *,aur,*) ;;
      *) continue ;;
    esac

    read_ini_section "$DIR"/conf/packages.ini "$section" | while IFS='' read -r package || [ -n "$package" ]
    do
      if [ -z "$package" ]; then
        continue
      fi

      if ! pacman -Qq "$package" >/dev/null 2>&1; then
        echo "$package"
      else
        log_verbose "Skipping AUR package $package: already installed" >&2
      fi
    done
  done | {
    packages=""
    while IFS='' read -r package
    do
      packages="$packages $package"
    done

    packages="$(echo "$packages" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"

    # Use common installation helper
    # Note: paru handles sudo internally, do not use sudo
    _install_package_list "$packages" "paru -S --needed --noconfirm" "Installing AUR packages" "aur_packages_installed"
  }
)}

# install_dotfiles_cli
#
# Create/update convenience symlink ~/.bin/dotfiles pointing to this repo's
# primary executable (dotfiles.sh). Avoids duplication if already correct.
install_dotfiles_cli()
{(
  # Check if the symlink already points to the correct location
  if [ "$(readlink -f "$DIR"/dotfiles.sh)" != "$(readlink -f "$HOME"/.bin/dotfiles)" ]; then
    log_stage "Installing dotfiles cli"
    if is_dry_run; then
      log_dry_run "Would create directory $HOME/.bin"
      log_dry_run "Would link $HOME/.bin/dotfiles to $DIR/dotfiles.sh"
    else
      log_verbose "Linking $HOME/.bin/dotfiles to $DIR/dotfiles.sh"
      # Ensure the bin directory exists
      mkdir -pv "$HOME"/.bin
      # Create the symlink, overwriting if necessary
      ln -snvf "$DIR"/dotfiles.sh "$HOME"/.bin/dotfiles
    fi
  else
    log_verbose "Skipping dotfiles cli installation: already linked"
  fi
)}

# install_paru
#
# Helper to install paru (AUR helper) if not present.
install_paru()
{(
  if is_program_installed "paru"; then
    log_verbose "Skipping paru installation: already installed"
    return
  fi

  # Check prerequisites
  if ! is_program_installed "git" || ! is_program_installed "makepkg" || ! is_program_installed "cargo"; then
    log_verbose "Skipping paru installation: missing prerequisites (install git, base-devel, rust)"
    return
  fi

  log_stage "Installing paru (AUR helper)"

  if is_dry_run; then
    log_dry_run "Would clone and build paru from AUR"
    return
  fi

  # Create temp directory and set up cleanup trap
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT
  log_verbose "Cloning paru-git to $tmp_dir"

  # Clone and build in the temp directory
  cd "$tmp_dir"
  git clone https://aur.archlinux.org/paru-git.git .
  log_verbose "Building paru..."
  makepkg -si --noconfirm
)}

# install_packages
#
# Install missing system packages (Arch pacman) from sections in packages.ini.
# Uses `--needed` so pacman skips already installed packages. Builds a single
# invocation for efficiency. Requires sudo + pacman presence.
install_packages()
{(
  if ! is_program_installed "sudo" || ! is_program_installed "pacman"; then
    log_verbose "Skipping package installation: sudo or pacman not installed (Arch Linux required)"
    return
  fi

  # Check if packages.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/packages.ini ]; then
    log_verbose "Skipping packages: no packages.ini found"
    return
  fi

  log_progress "Checking packages..."

  packages=""
  log_verbose "Processing packages from: conf/packages.ini"

  # Get list of sections from packages.ini
  # Sections use comma-separated categories (e.g., [arch], [arch,desktop])
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/packages.ini | tr -d '[]')"

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping packages section [$section]: profile not included" >&2
      continue
    fi

    # Skip AUR sections (they contain 'aur' tag)
    # These are handled by install_aur_packages
    case ",$section," in
      *,aur,*)
        log_verbose "Skipping packages section [$section]: AUR packages handled separately" >&2
        continue
        ;;
    esac

    # Read packages from this section
    read_ini_section "$DIR"/conf/packages.ini "$section" | while IFS='' read -r package || [ -n "$package" ]
    do
      if [ -z "$package" ]; then
        continue
      fi

      # Check if package is already installed (quietly)
      if ! pacman -Qq "$package" >/dev/null 2>&1; then
        echo "$package"
      else
        log_verbose "Skipping package $package: already installed" >&2
      fi
    done
  done | {
    packages=""
    while IFS='' read -r package
    do
      packages="$packages $package"
    done

    # Trim leading/trailing whitespace from packages list
    packages="$(echo "$packages" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"

    # Use common installation helper
    _install_package_list "$packages" "sudo pacman -S --quiet --needed --noconfirm" "Installing packages" "packages_installed"
  }
)}

# install_powershell_modules
#
# Defer to PowerShell helper to install required modules (Az, PSScriptAnalyzer)
# if pwsh is available. Keeps logic centralized in script.psm1.
install_powershell_modules()
{(
  # Check if PowerShell Core is installed
  if is_program_installed "pwsh"; then
    log_progress "Checking PowerShell modules..."
    args=""
    # Pass verbose flag if set
    if is_flag_set "v"; then
      args="-Verbose"
    fi
    # Pass dry run flag if set
    if is_dry_run; then
      args="$args -DryRun"
    fi
    # Import the helper module and run the installation function
    pwsh -Command "Import-Module $DIR/src/linux/script.psm1 && Install-PowerShellModules $args"
  else
    log_verbose "Skipping PowerShell modules: pwsh not installed"
  fi
)}

# install_symlinks
#
# Create/update symlinks listed in conf/symlinks.ini. Existing
# targets are removed (non-destructively; original file replaced by managed
# link). Creates parent directories when path contains '/'.
install_symlinks()
{(
  # Check if symlinks.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/symlinks.ini ]; then
    log_verbose "Skipping symlinks: no symlinks.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/symlinks.ini; then
    log_verbose "Skipping symlinks: no sections match active profile"
    return
  fi

  log_progress "Checking symlinks..."

  # Get list of sections from symlinks.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/symlinks.ini | tr -d '[]')"

  local act=0

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping symlinks section [$section]: profile not included"
      continue
    fi

    # Read symlinks from this section
    read_ini_section "$DIR"/conf/symlinks.ini "$section" | while IFS='' read -r symlink || [ -n "$symlink" ]
    do
      if [ -z "$symlink" ]; then
        continue
      fi

      # Check if source file exists (may be excluded by sparse checkout or missing)
      if [ ! -e "$DIR"/symlinks/"$symlink" ]; then
        # Distinguish between sparse checkout exclusion and genuinely missing files
        # Note: Check if file is tracked but not checked out (sparse checkout behavior)
        if [ -d "$DIR"/.git ] && git -C "$DIR" ls-files "symlinks/$symlink" 2>/dev/null | grep -q .; then
          log_verbose "Skipping symlink $symlink: source excluded by sparse checkout"
        else
          log_verbose "Skipping symlink $symlink: source file missing (possible configuration error)"
        fi
        continue
      fi

      # Check if symlink is already correctly pointing to the target
      if ! is_symlink_installed "$symlink"; then
        if [ $act -eq 0 ]; then
          act=1
          log_stage "Installing symlinks"
        fi
        if is_dry_run; then
          log_dry_run "Would ensure parent directory: $(dirname "$HOME"/."$symlink")"
          if [ -e "$HOME"/."$symlink" ]; then
            log_dry_run "Would remove existing: $HOME/.$symlink"
          fi
          log_dry_run "Would link $DIR/symlinks/$symlink to $HOME/.$symlink"
          increment_counter "symlinks_created"
        else
          log_verbose "Linking $DIR/symlinks/$symlink to $HOME/.$symlink"
          # Ensure parent directory exists
          mkdir -p "$(dirname "$HOME"/."$symlink")"

          # Remove existing file/directory/symlink if it exists (to replace with symlink)
          # Use -e for existing files/dirs and -L for symlinks (including broken ones)
          if [ -e "$HOME"/."$symlink" ] || [ -L "$HOME"/."$symlink" ]; then
            rm -rf "$HOME"/."$symlink"
          fi

          # Create the symlink
          ln -snf "$DIR"/symlinks/"$symlink" "$HOME"/."$symlink"
          increment_counter "symlinks_created"
        fi
      else
        log_verbose "Skipping symlink $symlink: already correct"
      fi
    done
  done
)}

# install_vscode_extensions
#
# Ensure VS Code / Code - Insiders extensions listed in vscode-extensions.ini are
# installed. Enumerates existing extensions once per binary to minimize
# process overhead. Installs missing ones individually (VS Code has no batch).
# Supports profile-based sections for filtering extensions by category.
install_vscode_extensions()
{(
  # Check if vscode-extensions.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/vscode-extensions.ini ]; then
    log_verbose "Skipping VS Code extensions: no vscode-extensions.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/vscode-extensions.ini; then
    log_verbose "Skipping VS Code extensions: no sections match active profile"
    return
  fi

  # Check if VS Code is installed
  if ! is_program_installed "code" && ! is_program_installed "code-insiders"; then
    log_verbose "Skipping VS Code extensions: code not installed (install VS Code or Code Insiders)"
    return
  fi

  log_progress "Checking VS Code extensions..."

  # Get list of sections from vscode-extensions.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/vscode-extensions.ini | tr -d '[]')"

  # Iterate over both stable and insiders versions of VS Code
  for code in code code-insiders
  do
    # Check if the code binary exists
    if ! is_program_installed "$code"; then
      continue
    fi

    # Get list of currently installed extensions to avoid redundant calls
    extensions="$($code --list-extensions)"

    # Check if any extensions need installing
    local tmpfile
    tmpfile="$(mktemp)"
    trap 'rm -f "$tmpfile"' EXIT
    for section in $sections
    do
      # Check if this section/profile should be included
      if ! should_include_profile_tag "$section"; then
        log_verbose "Skipping VS Code extensions section [$section]: profile not included" >&2
        continue
      fi

      # Read extensions from this section
      read_ini_section "$DIR"/conf/vscode-extensions.ini "$section" | while IFS='' read -r extension || [ -n "$extension" ]
      do
        if [ -n "$extension" ] && ! echo "$extensions" | grep -qxF "$extension"; then
          echo "$extension"
        fi
      done
    done | sort -u > "$tmpfile"

    if [ -s "$tmpfile" ]; then
      log_stage "Installing $code extensions"

      # Now install the missing extensions
      while IFS='' read -r extension || [ -n "$extension" ]
      do
        if is_dry_run; then
          log_dry_run "Would install extension: $extension"
          increment_counter "vscode_extensions_installed"
        else
          log_verbose "Installing extension: $extension"
          $code --install-extension "$extension"
          increment_counter "vscode_extensions_installed"
        fi
      done < "$tmpfile"
    fi

    rm -f "$tmpfile"

    # Log already installed extensions
    for section in $sections
    do
      # Check if this section/profile should be included
      if ! should_include_profile_tag "$section"; then
        continue
      fi

      read_ini_section "$DIR"/conf/vscode-extensions.ini "$section" | while IFS='' read -r extension || [ -n "$extension" ]
      do
        if [ -n "$extension" ] && echo "$extensions" | grep -qxF "$extension"; then
          log_verbose "Skipping $code extension $extension: already installed"
        fi
      done
    done
  done
)}

# install_copilot_skills
#
# Download and install GitHub Copilot CLI skill folders from GitHub URLs listed
# in copilot-skills.ini. Skills are downloaded to ~/.copilot/skills/ directory.
# Supports profile-based sections for filtering skills by category.
install_copilot_skills()
{(
  # Check if copilot-skills.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/copilot-skills.ini ]; then
    log_verbose "Skipping Copilot CLI skills: no copilot-skills.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/copilot-skills.ini; then
    log_verbose "Skipping Copilot CLI skills: no sections match active profile"
    return
  fi

  # Check if curl is available for downloading (required for GitHub API)
  if ! is_program_installed "curl"; then
    log_verbose "Skipping Copilot CLI skills: curl not installed"
    return
  fi

  # Check if jq is available for JSON parsing
  if ! is_program_installed "jq"; then
    log_verbose "Skipping Copilot CLI skills: jq not installed"
    return
  fi

  log_progress "Checking Copilot CLI skills..."

  # Get list of sections from copilot-skills.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/copilot-skills.ini | tr -d '[]')"

  # Ensure skills directory exists
  local skills_dir="$HOME/.copilot/skills"

  # Helper function to recursively download GitHub folder contents
  download_github_folder()
  {
    local owner="$1"
    local repo="$2"
    local branch="$3"
    local api_path="$4"
    local target_path="$5"
    local files_downloaded_var="$6"

    local api_url="https://api.github.com/repos/$owner/$repo/contents/$api_path?ref=$branch"
    log_verbose "Fetching contents from: $api_path"

    local temp_json
    temp_json="$(mktemp)"
    if ! curl -fsSL "$api_url" > "$temp_json"; then
      log_verbose "Failed to fetch contents from $api_url"
      rm -f "$temp_json"
      return
    fi

    # Process files
    jq -r '.[] | select(.type == "file") | .name + "|" + .download_url' "$temp_json" | while IFS='|' read -r file_name download_url
    do
      if [ -z "$file_name" ] || [ -z "$download_url" ]; then
        continue
      fi

      local file_path="$target_path/$file_name"
      local file_dir
      file_dir="$(dirname "$file_path")"

      # Ensure directory exists
      mkdir -p "$file_dir"

      log_verbose "Downloading file: $file_name"

      # Download to temporary file first
      local temp_file
      temp_file="$(mktemp)"
      if curl -fsSL "$download_url" > "$temp_file"; then
        # Check if file exists and content is different
        if [ -f "$file_path" ] && cmp -s "$temp_file" "$file_path"; then
          log_verbose "Skipping file $file_name: no changes"
          rm -f "$temp_file"
        else
          mv "$temp_file" "$file_path"
          local relative_path="${file_path#"$target_path"/}"
          log_verbose "Installed file: $relative_path"
          eval "$files_downloaded_var=\$((\$$files_downloaded_var + 1))"
        fi
      else
        log_verbose "Failed to download file from $download_url"
        rm -f "$temp_file"
      fi
    done

    # Process subdirectories recursively
    jq -r '.[] | select(.type == "dir") | .name + "|" + .path' "$temp_json" | while IFS='|' read -r dir_name dir_path
    do
      if [ -z "$dir_name" ] || [ -z "$dir_path" ]; then
        continue
      fi

      local sub_target_path="$target_path/$dir_name"
      log_verbose "Processing subdirectory: $dir_name"

      # Recursively download subdirectory
      download_github_folder "$owner" "$repo" "$branch" "$dir_path" "$sub_target_path" "$files_downloaded_var"
    done

    rm -f "$temp_json"
  }

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping Copilot CLI skills section [$section]: profile not included"
      continue
    fi

    # Read skill URLs from this section
    read_ini_section "$DIR"/conf/copilot-skills.ini "$section" | while IFS='' read -r url || [ -n "$url" ]
    do
      if [ -z "$url" ]; then
        continue
      fi

      # Parse GitHub URL to extract components
      # Example: https://github.com/user/repo/blob/main/path/folder
      #      or: https://github.com/user/repo/tree/main/path/folder
      if ! echo "$url" | grep -qE 'github\.com/[^/]+/[^/]+/(blob|tree)/[^/]+/.+'; then
        log_verbose "Invalid GitHub URL format: $url"
        continue
      fi

      owner="$(echo "$url" | sed -E 's|.*/github\.com/([^/]+)/.*|\1|')"
      repo="$(echo "$url" | sed -E 's|.*/github\.com/[^/]+/([^/]+)/.*|\1|')"
      branch="$(echo "$url" | sed -E 's|.*/(blob|tree)/([^/]+)/.*|\2|')"
      folder_path="$(echo "$url" | sed -E 's|.*/(blob|tree)/[^/]+/(.+)|\2|')"

      # Extract folder name from path (last segment)
      folder_name="$(basename "$folder_path")"
      target_dir="$skills_dir/$folder_name"

      if is_dry_run; then
        log_stage "Installing Copilot CLI skills"
        log_dry_run "Would create directory: $target_dir"
        log_dry_run "Would download skill folder from $url (including subdirectories)"
        increment_counter "copilot_skills_installed"
      else
        log_stage "Installing Copilot CLI skills"
        log_verbose "Downloading skill folder from $url"

        # Ensure target directory exists
        mkdir -p "$target_dir"

        files_downloaded=0

        # Recursively download folder contents
        download_github_folder "$owner" "$repo" "$branch" "$folder_path" "$target_dir" "files_downloaded"

        if [ $files_downloaded -gt 0 ]; then
          log_verbose "Installed skill: $folder_name ($files_downloaded file(s))"
          increment_counter "copilot_skills_installed"
        else
          log_verbose "Skipping skill $folder_name: no changes"
        fi
      fi
    done
  done
)}

# uninstall_symlinks
#
# Remove managed symlinks when present. Does not remove now-empty parent
# directories to avoid unintended cleanup of user-managed content.
uninstall_symlinks()
{(
  # Check if symlinks.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/symlinks.ini ]; then
    log_verbose "Skipping uninstall symlinks: no symlinks.ini found"
    return
  fi

  # Check if any sections match the active profile
  if ! has_matching_sections "$DIR"/conf/symlinks.ini; then
    log_verbose "Skipping uninstall symlinks: no sections match current profile"
    return
  fi

  log_progress "Checking symlinks to remove..."

  # Get list of sections from symlinks.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/symlinks.ini | tr -d '[]')"

  # Collect all symlinks that need to be removed to print stage header only once
  local tmpfile
  tmpfile="$(mktemp)"
  trap 'rm -f "$tmpfile"' EXIT
  for section in $sections
  do
    if should_include_profile_tag "$section"; then
      read_ini_section "$DIR"/conf/symlinks.ini "$section" | while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if [ -n "$symlink" ] && is_symlink_installed "$symlink"; then
          echo "$symlink"
        fi
      done
    fi
  done > "$tmpfile"

  if [ -s "$tmpfile" ]; then
    log_stage "Uninstalling symlinks"
  fi

  # Process each section that should be included
  for section in $sections
  do
    # Check if this section/profile should be included
    if ! should_include_profile_tag "$section"; then
      log_verbose "Skipping uninstall symlinks section [$section]: profile not included"
      continue
    fi

    # Read symlinks from this section
    read_ini_section "$DIR"/conf/symlinks.ini "$section" | while IFS='' read -r symlink || [ -n "$symlink" ]
    do
      # Skip empty lines
      if [ -z "$symlink" ]; then
        continue
      fi

      # Check if the symlink is currently installed
      if is_symlink_installed "$symlink"; then
        if is_dry_run; then
          log_dry_run "Would remove symlink: $HOME/.$symlink"
          increment_counter "symlinks_removed"
        else
          log_verbose "Removing symlink: $HOME/.$symlink"
          # Remove the symlink
          rm -f "$HOME"/."$symlink"
          increment_counter "symlinks_removed"
        fi
      else
        log_verbose "Skipping uninstall symlink $symlink: not installed"
      fi
    done
  done
)}

# update_dotfiles
#
# Fetch + merge remote changes when local working tree is clean, current
# branch matches remote HEAD, and upstream has diverged. Uses a conservative
# sequence: fetch only when remote changed, then merge if commit hashes differ.
update_dotfiles()
{(
  # Check if this is a git repository
  if [ ! -d "$DIR"/.git ]; then
    log_verbose "Skipping update dotfiles: not a git repository"
    return
  fi
  # Check if git is installed
  if ! is_program_installed "git"; then
    log_verbose "Skipping update dotfiles: git not installed"
    return
  fi

  # Ensure working tree is clean before attempting update
  if ! git -C "$DIR" diff-index --quiet HEAD --; then
    log_verbose "Skipping update dotfiles: working tree not clean"
    return
  fi

  # Ensure we are on the same branch as the remote HEAD
  # Check if origin/HEAD exists (it may not in CI or shallow clones)
  if git -C "$DIR" rev-parse --verify --quiet origin/HEAD >/dev/null 2>&1; then
    local current_branch
    local remote_branch
    current_branch="$(git -C "$DIR" rev-parse --abbrev-ref HEAD)"
    remote_branch="$(git -C "$DIR" rev-parse --abbrev-ref origin/HEAD)"
    # Strip 'origin/' prefix from remote branch name
    remote_branch="${remote_branch#origin/}"

    if [ "$remote_branch" != "$current_branch" ]; then
      log_verbose "Skipping update dotfiles: current branch '$current_branch' does not match origin/HEAD '$remote_branch'"
      return
    fi
  else
    log_verbose "Skipping update dotfiles: origin/HEAD not found (shallow clone or detached HEAD)"
    return
  fi

  # Always fetch to ensure we have latest remote refs
  # git fetch --dry-run produces no output, so we can't use it to detect changes
  if is_dry_run; then
    log_dry_run "Would fetch updates from origin"
  else
    log_verbose "Fetching updates from origin"
    git -C "$DIR" fetch
  fi

  # Check if the local HEAD is behind the remote HEAD
  # Only proceed if origin/HEAD exists
  if git -C "$DIR" rev-parse --verify --quiet origin/HEAD >/dev/null 2>&1; then
    if [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]; then
      log_stage "Updating dotfiles"
      if is_dry_run; then
        log_dry_run "Would merge updates from origin/HEAD to $current_branch"
      else
        log_verbose "Merging updates from origin/HEAD to $current_branch"
        git -C "$DIR" merge
      fi
    else
      log_verbose "Skipping merge: HEAD is up to date with origin/HEAD"
    fi
  else
    log_verbose "Skipping merge: origin/HEAD not found (shallow clone or detached HEAD)"
  fi
)}

# install_repository_git_hooks
#
# Install git hooks for this dotfiles repository as symlinks.
# Hooks are stored in the hooks/ directory and symlinked into .git/hooks/
# so that updates to the hook files are automatically reflected.
#
# Implementation Notes:
#   * Only installs hooks for this repository (not user's git templates)
#   * Creates symlinks so hook updates don't require reinstallation
#   * Makes hook files executable before symlinking
#   * Skips if not a git repository or if hooks don't exist
install_repository_git_hooks()
{(
  # Check if this is a git repository
  if [ ! -d "$DIR"/.git ]; then
    log_verbose "Skipping git hooks: not a git repository"
    return
  fi

  # Check if hooks directory exists
  if [ ! -d "$DIR"/hooks ]; then
    log_verbose "Skipping git hooks: hooks directory not found"
    return
  fi

  local act=0

  # Ensure .git/hooks directory exists
  if [ ! -d "$DIR/.git/hooks" ]; then
    if [ $act -eq 0 ]; then
      act=1
      log_stage "Installing repository git hooks"
    fi
    if is_dry_run; then
      log_dry_run "Would create directory: .git/hooks"
    else
      log_verbose "Creating directory: .git/hooks"
      mkdir -p "$DIR/.git/hooks"
    fi
  fi

  # Process all files in hooks/ directory, excluding non-hook files
  for hook_file in "$DIR"/hooks/*; do
    # Skip if not a regular file
    if [ ! -f "$hook_file" ]; then
      continue
    fi

    hook_name="$(basename "$hook_file")"

    # Skip non-hook files (config files, documentation, hidden files)
    case "$hook_name" in
      *.md|*.txt|*.ini|*.yaml|*.yml|*.json|README|.*)
        log_verbose "Skipping non-hook file: $hook_name"
        continue
        ;;
    esac

    # Target location in .git/hooks/
    target="$DIR/.git/hooks/$hook_name"

    # Make source hook executable
    if [ ! -x "$hook_file" ]; then
      if [ $act -eq 0 ]; then
        act=1
        log_stage "Installing repository git hooks"
      fi
      if is_dry_run; then
        log_dry_run "Would make executable: hooks/$hook_name"
      else
        log_verbose "Making executable: hooks/$hook_name"
        chmod +x "$hook_file"
      fi
    fi

    # Check if symlink already exists and points to correct location
    if [ -L "$target" ] && [ "$(readlink "$target")" = "$hook_file" ]; then
      log_verbose "Skipping hook $hook_name: already installed"
      continue
    fi

    # Install the hook as a symlink
    if [ $act -eq 0 ]; then
      act=1
      log_stage "Installing repository git hooks"
    fi
    if is_dry_run; then
      log_dry_run "Would install hook: $hook_name"
    else
      log_verbose "Installing hook: $hook_name"
      # Remove existing file/symlink if present
      rm -f "$target"
      # Create symlink using absolute path
      ln -s "$hook_file" "$target"
    fi
  done
)}


