#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
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

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

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
      # Use read with proper word splitting to handle tabs and multiple spaces
      mode="$(echo "$line" | awk '{print $1}')"
      file="$(echo "$line" | cut -d' ' -f2- | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"

      if [ -z "$mode" ] || [ -z "$file" ]; then
        continue
      fi

      target=~/."$file"

      # Check if the target file exists - skip gracefully if not
      if [ ! -e "$target" ]; then
        log_verbose "Skipping chmod on $target: file does not exist"
        continue
      fi

      # Check if mode is already correct
      # Use -L to dereference symlinks and check the target file's permissions
      current_mode="$(stat -L -c '%a' "$target" 2>/dev/null || echo '')"
      if [ "$current_mode" = "$mode" ]; then
        # For directories, we need to check if ALL contents have correct permissions
        if [ -d "$target" ]; then
          # Check if any file in the directory tree has incorrect permissions
          if find "$target" -L ! -perm "$mode" 2>/dev/null | grep -q .; then
            # Some files have wrong permissions
            :
          else
            # All files have correct permissions
            log_verbose "Skipping chmod on $target: permissions already correct"
            continue
          fi
        else
          # File has correct permissions
          log_verbose "Skipping chmod on $target: permissions already correct"
          continue
        fi
      fi

      # Apply chmod
      if [ $act -eq 0 ]; then
        act=1
        log_stage "Configuring file permissions"
      fi
      if is_dry_run; then
        log_dry_run "Would set mode $mode on $target"
      else
        log_verbose "Setting mode $mode on $target"
        # Note: -R flag applies mode recursively to ALL files/directories
        chmod -c -R "$mode" "$target"
      fi
    done
  done
)}

# configure_fonts
#
# Refresh font cache when GUI fonts list (conf/fonts.ini) differs from currently
# installed families. Skips if: required fc-* tools missing, fonts.ini excluded
# by sparse checkout, or all listed font families already present.
configure_fonts()
{(
  # Check if font configuration tools are installed
  if ! is_program_installed "fc-list" || ! is_program_installed "fc-cache"; then
    log_verbose "Skipping font configuration: fc-list or fc-cache not installed"
    return
  fi

  # Check if fonts.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/fonts.ini ]; then
    log_verbose "Skipping font configuration: no fonts.ini found"
    return
  fi

  missing_fonts=0

  log_verbose "Checking fonts from: conf/fonts.ini"

  # Read the list of required fonts from the [fonts] section
  # Check if any fonts are missing using a temp file for POSIX compliance
  tmpfile=$(mktemp)
  read_ini_section "$DIR"/conf/fonts.ini "fonts" > "$tmpfile"

  missing_fonts=0
  while IFS='' read -r font || [ -n "$font" ]; do
    if [ -z "$font" ]; then
      continue
    fi

    # Check if the font family is already installed in the system
    # fc-list outputs comma-separated family names, so we need to handle that
    # Convert commas to newlines and check for exact match of any family name
    if ! fc-list : family | tr ',' '\n' | grep -Fxq "$font"; then
      missing_fonts=1
      break
    fi
  done < "$tmpfile"
  rm -f "$tmpfile"

  if [ "$missing_fonts" -eq 0 ]; then
    log_verbose "Skipping font configuration: fonts already up to date"
    return
  fi

  log_stage "Updating fonts"
  if is_dry_run; then
    log_dry_run "Would run fc-cache to update font cache"
  else
    log_verbose "Running fc-cache to update font cache"
    fc-cache
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

  log_verbose "Processing systemd units from: conf/units.ini"

  # Get list of sections from units.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/units.ini | tr -d '[]')"

  # Collect all units that need to be enabled across all sections
  # to print stage header only once
  tmpfile="$(mktemp)"
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
        else
          log_verbose "Enabling systemd unit: $unit"
          systemctl --user enable "$unit"

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

# install_dotfiles_cli
#
# Create/update convenience symlink ~/.bin/dotfiles pointing to this repo's
# primary executable (dotfiles.sh). Avoids duplication if already correct.
install_dotfiles_cli()
{(
  # Check if the symlink already points to the correct location
  if [ "$(readlink -f "$DIR"/dotfiles.sh)" != "$(readlink -f ~/.bin/dotfiles)" ]; then
    log_stage "Installing dotfiles cli"
    if is_dry_run; then
      log_dry_run "Would create directory ~/.bin"
      log_dry_run "Would link ~/.bin/dotfiles to $DIR/dotfiles.sh"
    else
      log_verbose "Linking ~/.bin/dotfiles to $DIR/dotfiles.sh"
      # Ensure the bin directory exists
      mkdir -pv ~/.bin
      # Create the symlink, overwriting if necessary
      ln -snvf "$DIR"/dotfiles.sh ~/.bin/dotfiles
    fi
  else
    log_verbose "Skipping dotfiles cli installation: already linked"
  fi
)}

# install_packages
#
# Install missing system packages (Arch pacman) from sections in packages.ini.
# Uses `--needed` so pacman skips already installed packages. Builds a single
# invocation for efficiency. Requires sudo + pacman presence.
install_packages()
{(
  if ! is_program_installed "sudo" || ! is_program_installed "pacman"; then
    log_verbose "Skipping package installation: sudo or pacman not installed"
    return
  fi

  # Check if packages.ini exists (may be excluded by sparse checkout)
  if [ ! -f "$DIR"/conf/packages.ini ]; then
    log_verbose "Skipping packages: no packages.ini found"
    return
  fi

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
      log_verbose "Skipping packages section [$section]: profile not included"
      continue
    fi

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

    if [ -n "$packages" ]; then
      log_stage "Installing packages"
      if is_dry_run; then
        log_dry_run "Would install packages: $packages"
      else
        log_verbose "Installing packages: $packages"
        # shellcheck disable=SC2086  # Word splitting intentional: $packages is space-separated list
        sudo pacman -S --quiet --needed --noconfirm $packages
      fi
    fi
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
          log_dry_run "Would ensure parent directory: $(dirname ~/".$symlink")"
          if [ -e ~/".$symlink" ]; then
            log_dry_run "Would remove existing: ~/.$symlink"
          fi
          log_dry_run "Would link $DIR/symlinks/$symlink to ~/.$symlink"
        else
          log_verbose "Linking $DIR/symlinks/$symlink to ~/.$symlink"
          # Ensure parent directory exists
          mkdir -pv "$(dirname ~/".$symlink")"

          # Remove existing file/directory if it exists (to replace with symlink)
          if [ -e ~/".$symlink" ]; then
            rm -rvf ~/".$symlink"
          fi

          # Create the symlink
          ln -snvf "$DIR"/symlinks/"$symlink" ~/".$symlink"
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
    extensions=$($code --list-extensions)

    # Check if any extensions need installing
    tmpfile="$(mktemp)"
    for section in $sections
    do
      # Check if this section/profile should be included
      if ! should_include_profile_tag "$section"; then
        log_verbose "Skipping VS Code extensions section [$section]: profile not included"
        continue
      fi

      # Read extensions from this section
      read_ini_section "$DIR"/conf/vscode-extensions.ini "$section" | while IFS='' read -r extension || [ -n "$extension" ]
      do
        if [ -n "$extension" ] && ! echo "$extensions" | grep -qw "$extension"; then
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
        else
          log_verbose "Installing extension: $extension"
          $code --install-extension "$extension"
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
        if [ -n "$extension" ] && echo "$extensions" | grep -qw "$extension"; then
          log_verbose "Skipping $code extension $extension: already installed"
        fi
      done
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

  # Get list of sections from symlinks.ini
  sections="$(grep -E '^\[.+\]$' "$DIR"/conf/symlinks.ini | tr -d '[]')"

  # Collect all symlinks that need to be removed to print stage header only once
  tmpfile="$(mktemp)"
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
          log_dry_run "Would remove symlink: ~/.$symlink"
        else
          log_verbose "Removing symlink: ~/.$symlink"
          # Remove the symlink
          rm -vf ~/".$symlink"
        fi
      else
        log_verbose "Skipping uninstall symlink $symlink: not installed"
      fi
    done
  done

  rm -f "$tmpfile"
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
    if [ "$(git -C "$DIR" rev-parse --abbrev-ref origin/HEAD | cut -d/ -f2)" != "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ]; then
      log_verbose "Skipping update dotfiles: current branch does not match origin/HEAD"
      return
    fi
  else
    log_verbose "Skipping update dotfiles: origin/HEAD not found (shallow clone or detached HEAD)"
    return
  fi

  # Check if there are changes to fetch
  if [ -n "$(git -C "$DIR" fetch --dry-run)" ]; then
    log_stage "Updating dotfiles"
    if is_dry_run; then
      log_dry_run "Would fetch updates from origin"
    else
      log_verbose "Fetching updates from origin"
      git -C "$DIR" fetch
    fi
  else
    log_verbose "Skipping fetch: no updates from origin"
  fi

  # Check if the local HEAD is behind the remote HEAD
  # Only proceed if origin/HEAD exists
  if git -C "$DIR" rev-parse --verify --quiet origin/HEAD >/dev/null 2>&1; then
    if [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]; then
      log_stage "Updating dotfiles"
      if is_dry_run; then
        log_dry_run "Would merge updates from origin/HEAD"
      else
        log_verbose "Merging updates from origin/HEAD"
        git -C "$DIR" merge
      fi
    else
      log_verbose "Skipping merge: HEAD is up to date with origin/HEAD"
    fi
  else
    log_verbose "Skipping merge: origin/HEAD not found (shallow clone or detached HEAD)"
  fi
)}


