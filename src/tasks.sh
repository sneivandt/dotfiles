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
#   logger.sh (log_stage, log_error)
#   utils.sh  (is_flag_set, is_env_ignored, is_program_installed, etc.)
# -----------------------------------------------------------------------------

. "$DIR"/src/logger.sh
. "$DIR"/src/utils.sh

# configure_file_mode_bits
#
# Apply chmod directives declared in each environment's chmod.conf.
# Format per line: <mode> <relative-path-under-home>
# Example: 600 ssh/config
#
# Implementation Notes:
#   * Reads each file line safely (including last line w/o newline).
#   * Uses -R allowing directories to be targeted; user path is prefixed with
#     a dot (".") to match symlink convention.
configure_file_mode_bits()
{(
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/chmod.conf ]
    then
      while IFS='' read -r line || [ -n "$line" ]
      do
        mode="$(echo "$line" | cut -d" " -f1)"
        target=~/."$(echo "$line" | cut -d" " -f2)"
        if [ -e "$target" ] \
          && [ -n "$(find -H "$target" ! -type l ! -perm "$mode" -print -quit 2>/dev/null)" ]
        then
          log_verbose "Setting mode $mode on $target"
          chmod -c -R "$mode" "$target"
        fi
      done < "$env"/chmod.conf
    fi
  done
)}

# configure_fonts
#
# Refresh font cache when GUI fonts list (fonts.conf) differs from currently
# installed families. Skips if: GUI env ignored, required fc-* tools missing,
# or all listed font families already present.
configure_fonts()
{(
  if ! is_env_ignored "arch-gui" \
    && is_program_installed "fc-list" \
    && is_program_installed "fc-cache" \
    && [ "$(fc-list : family | grep -f "$DIR"/env/arch-gui/fonts.conf -cx)" != "$(grep -c "" "$DIR"/env/arch-gui/fonts.conf | cut -d" " -f1)" ]
  then
    log_stage "Updating fonts"
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
  if is_program_installed "zsh" \
    && [ "$SHELL" != "$(zsh -c "command -vp zsh")" ] \
    && [ ! -f /.dockerenv ] \
    && [ "$(passwd --status "$USER" | cut -d" " -f2)" = "P" ]
  then
    log_stage "Configuring user shell"
    log_verbose "Changing shell to $(zsh -c "command -vp zsh")"
    chsh -s "$(zsh -c "command -vp zsh")"
  fi
)}

# configure_systemd
#
# Enable (and start when user session active) user-level systemd units listed
# in each environment's units.conf when -s flag provided. Only units already
# installed (list-unit-files) are considered. Avoids starting during early
# boot by checking `systemctl is-system-running` state.
configure_systemd()
{(
  if is_flag_set "s" \
    && [ "$(ps -p 1 -o comm=)" = "systemd" ] \
    && is_program_installed "systemctl"
  then
    for env in "$DIR"/env/*
    do
      if ! is_env_ignored "$(basename "$env")" \
        && [ -e "$env"/units.conf ]
      then
        while IFS='' read -r unit || [ -n "$unit" ]
        do
          if systemctl --user list-unit-files | cut -d" " -f1 | grep -qx "$unit" \
            && ! systemctl --user is-enabled --quiet "$unit"
          then
            log_stage "Configuring systemd"
            log_verbose "Enabling systemd unit: $unit"
            systemctl --user enable "$unit"
            if [ "$(systemctl is-system-running)" = "running" ]
            then
              log_verbose "Starting systemd unit: $unit"
              systemctl --user start "$unit"
            fi
          fi
        done < "$env"/units.conf
      fi
    done
  fi
)}

# install_dotfiles_cli
#
# Create/update convenience symlink ~/.bin/dotfiles pointing to this repo's
# primary executable (dotfiles.sh). Avoids duplication if already correct.
install_dotfiles_cli()
{(
  if [ "$(readlink -f "$DIR"/dotfiles.sh)" != "$(readlink -f ~/.bin/dotfiles)" ]
  then
    log_stage "Installing dotfiles cli"
    log_verbose "Linking ~/.bin/dotfiles to $DIR/dotfiles.sh"
    mkdir -pv ~/.bin
    ln -snvf "$DIR"/dotfiles.sh ~/.bin/dotfiles
  fi
)}

# install_git_submodules
#
# Initialize any git submodules declared for base + active environments when
# status indicates they are uninitialized or out of date (+ or - markers).
# Reads base/submodules.conf then appends env paths (env/<name>). Uses
# recursive init to support nested submodules.
install_git_submodules()
{(
  if [ -d "$DIR"/.git ] \
    && is_program_installed "git"
  then
    modules="$(cat "$DIR"/env/base/submodules.conf)"
    for env in "$DIR"/env/*
    do
      if [ "$(basename "$env")" != "base" ] \
        && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules "env/$(basename "$env")
      fi
    done
    # shellcheck disable=SC2086
    if git -C "$DIR" submodule status $modules | cut -c-1 | grep -q "+\\|-"
    then
      log_stage "Installing git submodules"
      log_verbose "Updating submodules: $modules"
      # shellcheck disable=SC2086
      git -C "$DIR" submodule update --init --recursive $modules
    fi
  fi
)}

# install_packages
#
# Install missing system packages (Arch pacman) aggregated from all active
# environments' packages.conf files when -p flag set. Uses `--needed` so
# pacman skips already installed packages. Builds a single invocation for
# efficiency. Requires sudo + pacman presence.
install_packages()
{(
  if is_flag_set "p" \
    && is_program_installed "sudo" \
    && is_program_installed "pacman"
  then
    packages=""
    for env in "$DIR"/env/*
    do
      if ! is_env_ignored "$(basename "$env")" \
        && [ -e "$env"/packages.conf ]
      then
        while IFS='' read -r package || [ -n "$package" ]
        do
          if ! pacman -Qq "$package" >/dev/null 2>&1
          then
            packages="$packages $package"
          fi
        done < "$env"/packages.conf
      fi
    done
    if [ -n "$packages" ]
    then
      log_stage "Installing packages"
      log_verbose "Installing packages: $packages"
      # shellcheck disable=SC2086
      sudo pacman -S --quiet --needed $packages
    fi
  fi
)}

# install_powershell_modules
#
# Defer to PowerShell helper to install required modules (Az, PSScriptAnalyzer)
# if pwsh is available. Keeps logic centralized in script.psm1 for Windows
# parity and test reuse.
install_powershell_modules()
{(
  if is_program_installed "pwsh"
  then
    args=""
    if is_flag_set "v"; then
      args="-Verbose"
    fi
    pwsh -Command "Import-Module $DIR/src/script.psm1 && Install-PowerShellModules $args"
  fi
)}

# install_symlinks
#
# Create/update symlinks listed in each environment's symlinks.conf. Existing
# targets are removed (non-destructively; original file replaced by managed
# link). Creates parent directories when path contains '/'.
install_symlinks()
{(
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if ! is_symlink_installed "$(basename "$env")" "$symlink"
        then
          log_stage "Installing symlinks"
          log_verbose "Linking $env/symlinks/$symlink to ~/.$symlink"
          case "$symlink" in
            *"/"*) mkdir -pv ~/."$(echo "$symlink" | rev | cut -d/ -f2- | rev)"
          esac
          if [ -e ~/."$symlink" ]
          then
            rm -rvf ~/."$symlink"
          fi
          ln -snvf "$env"/symlinks/"$symlink" ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
)}

# install_vscode_extensions
#
# Ensure VS Code / Code - Insiders extensions listed in base-gui config are
# installed. Enumerates existing extensions once per binary to minimize
# process overhead. Installs missing ones individually (VS Code has no batch).
install_vscode_extensions()
{(
  for code in code code-insiders
  do
    if ! is_env_ignored "base-gui" \
      && is_program_installed "$code"
    then
      extensions=$($code --list-extensions)
      while IFS='' read -r extension || [ -n "$extension" ]
      do
        if ! echo "$extensions" | grep -qw "$extension"
        then
          log_stage "Installing $code extensions"
          log_verbose "Installing extension: $extension"
          $code --install-extension "$extension"
        fi
      done < "$DIR/env/base-gui/vscode-extensions.conf"
    fi
  done
)}

# test_psscriptanalyzer
#
# Run PowerShell static analysis across repo when pwsh + analyzer module
# available. Skips silently otherwise to keep CI resilient on systems without
# PowerShell.
test_psscriptanalyzer()
{(
  if is_program_installed "pwsh"
  then
    log_verbose "Running PSScriptAnalyzer"
    pwsh -Command "Import-Module $DIR/src/script.psm1 && Test-PSScriptAnalyzer -dir $DIR"
  fi
)}

# test_shellcheck
#
# Execute shellcheck across all shell scripts discovered through env symlink
# trees excluding any paths that reside within declared submodules (to avoid
# flagging third-party code). Non-zero shellcheck exit is swallowed (|| true)
# so the overall run continues; individual findings still surface.
test_shellcheck()
{(
  if ! is_program_installed "shellcheck"
  then
    log_error "shellcheck not installed"
  else
    log_stage "Running shellcheck"
    scripts="$DIR"/dotfiles.sh
    for env in "$DIR"/env/*
    do
      if [ -e "$env"/symlinks.conf ]
      then
        while IFS='' read -r symlink || [ -n "$symlink" ]
        do
          if [ -d "$env"/symlinks/"$symlink" ]
          then
            tmpfile="$(mktemp)"
            find "$env"/symlinks/"$symlink" -type f > "$tmpfile"
            while IFS='' read -r line || [ -n "$line" ]
            do
              ignore=false
              if [ -e "$env"/submodules.conf ]
              then
                while IFS='' read -r submodule || [ -n "$submodule" ]
                do
                  case "$line" in
                    "$DIR"/"$submodule"/*)
                      ignore=true
                      ;;
                  esac
                done < "$env"/submodules.conf
              fi
              if ! "$ignore" \
                && is_shell_script "$line"
              then
                scripts="$scripts $line"
              fi
            done < "$tmpfile"
            rm "$tmpfile"
          elif is_shell_script "$env"/symlinks/"$symlink"
          then
            scripts="$scripts $env"/symlinks/"$symlink"
          fi
        done < "$env"/symlinks.conf
      fi
    done
    # shellcheck disable=SC2086
    log_verbose "Checking scripts: $scripts"
    shellcheck $scripts || true
  fi
)}

# uninstall_symlinks
#
# Remove managed symlinks when present. Does not remove now-empty parent
# directories to avoid unintended cleanup of user-managed content.
uninstall_symlinks()
{(
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/symlinks.conf ]
    then
      while IFS='' read -r symlink || [ -n "$symlink" ]
      do
        if is_symlink_installed "$env" "$symlink"
        then
          log_stage "Uninstalling symlinks"
          log_verbose "Removing symlink: ~/.$symlink"
          rm -vf ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
)}

# update_dotfiles
#
# Fetch + merge remote changes when local working tree is clean, current
# branch matches remote HEAD, and upstream has diverged. Uses a conservative
# sequence: fetch only when remote changed, then merge if commit hashes differ.
update_dotfiles()
{(
  if [ -d "$DIR"/.git ] \
    && is_program_installed "git" \
    && git -C "$DIR" diff-index --quiet HEAD -- \
    && [ "$(git -C "$DIR" rev-parse --abbrev-ref origin/HEAD | cut -d/ -f2)" = "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ]
  then
    if [ -n "$(git -C "$DIR" fetch --dry-run)" ]
    then
      log_stage "Updating dotfiles"
      log_verbose "Fetching updates from origin"
      git -C "$DIR" fetch
    fi
    if [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
    then
      log_stage "Updating dotfiles"
      log_verbose "Merging updates from origin/HEAD"
      git -C "$DIR" merge
    fi
  fi
)}

# update_git_submodules
#
# Update git submodules for active environments (excluding base) pulling
# latest remote commits ( --remote ) for tracking branches. Skips when status
# output is non-empty (indicates uninitialized or modified state where an
# install pass should happen first). Ensures recursive consistency.
update_git_submodules()
{(
  if [ -d "$DIR"/.git ] \
    && is_program_installed "git"
  then
    modules=""
    for env in "$DIR"/env/*
    do
      if [ "$(basename "$env")" != "base" ] \
        && ! is_env_ignored "$(basename "$env")"
      then
        modules="$modules env/"$(basename "$env")
      fi
    done
    # shellcheck disable=SC2086
    if [ -z "$(git -C "$DIR" submodule status $modules | cut -c1)" ]
    then
      log_stage "Updating git submodules"
      log_verbose "Updating submodules: $modules"
      # shellcheck disable=SC2086
      git -C "$DIR" submodule update --init --recursive --remote $modules
    fi
  fi
)}
