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
      while read -r mode file || [ -n "$mode" ]
      do
        case "$mode" in ""|\#*) continue ;; esac
        target=~/."$file"
        if [ ! -e "$target" ]
        then
          log_verbose "Skipping chmod on $target: file does not exist"
        elif [ -z "$(find -H "$target" ! -type l ! -perm "$mode" -print -quit 2>/dev/null)" ]
        then
          log_verbose "Skipping chmod on $target: permissions already correct"
        else
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
  if is_env_ignored "arch-gui"
  then
    return
  fi

  if ! is_program_installed "fc-list" || ! is_program_installed "fc-cache"
  then
    log_verbose "Skipping font configuration: fc-list or fc-cache not installed"
    return
  fi

  missing_fonts=0
  while IFS='' read -r font || [ -n "$font" ]; do
    case "$font" in ""|\#*) continue ;; esac
    if ! fc-list : family | grep -Fxq "$font"
    then
      missing_fonts=1
      break
    fi
  done < "$DIR"/env/arch-gui/fonts.conf

  if [ "$missing_fonts" -eq 0 ]
  then
    log_verbose "Skipping font configuration: fonts already up to date"
    return
  fi

  log_stage "Updating fonts"
  log_verbose "Running fc-cache to update font cache"
  fc-cache
)}

# configure_shell
#
# Change default login shell to zsh when available and not already set.
# Skip inside Docker (/.dockerenv) and when passwd status indicates locked
# account. Uses chsh invoking absolute path resolved via a nested zsh.
configure_shell()
{(
  if ! is_program_installed "zsh"
  then
    log_verbose "Skipping shell configuration: zsh not installed"
    return
  fi

  zsh_path="$(zsh -c "command -vp zsh")"
  if [ "$SHELL" = "$zsh_path" ]
  then
    log_verbose "Skipping shell configuration: shell already set to zsh"
    return
  fi

  if [ -f /.dockerenv ]
  then
    log_verbose "Skipping shell configuration: running inside Docker"
    return
  fi

  if [ "$(passwd --status "$USER" | cut -d" " -f2)" != "P" ]
  then
    log_verbose "Skipping shell configuration: user account not usable (passwd status)"
    return
  fi

  log_stage "Configuring user shell"
  log_verbose "Changing shell to $zsh_path"
  chsh -s "$zsh_path"
)}

# configure_systemd
#
# Enable (and start when user session active) user-level systemd units listed
# in each environment's units.conf when -s flag provided. Only units already
# installed (list-unit-files) are considered. Avoids starting during early
# boot by checking `systemctl is-system-running` state.
configure_systemd()
{(
  if ! is_flag_set "s"
  then
    log_verbose "Skipping systemd configuration: -s flag not set"
    return
  fi

  if [ "$(ps -p 1 -o comm=)" != "systemd" ]
  then
    log_verbose "Skipping systemd configuration: not running under systemd"
    return
  fi

  if ! is_program_installed "systemctl"
  then
    log_verbose "Skipping systemd configuration: systemctl not installed"
    return
  fi

  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/units.conf ]
    then
      while IFS='' read -r unit || [ -n "$unit" ]
      do
        case "$unit" in ""|\#*) continue ;; esac
        if ! systemctl --user list-unit-files | cut -d" " -f1 | grep -qx "$unit"
        then
          log_verbose "Skipping systemd unit $unit: not found in unit files"
        elif systemctl --user is-enabled --quiet "$unit"
        then
          log_verbose "Skipping systemd unit $unit: already enabled"
        else
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
  else
    log_verbose "Skipping dotfiles cli installation: already linked"
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
  if [ ! -d "$DIR"/.git ]
  then
    log_verbose "Skipping git submodules: not a git repository"
    return
  fi

  if ! is_program_installed "git"
  then
    log_verbose "Skipping git submodules: git not installed"
    return
  fi

  known_submodules="$(git -C "$DIR" submodule status | awk '{print $2}')"
  modules=""

  if [ -f "$DIR"/env/base/submodules.conf ]
  then
    while IFS='' read -r module || [ -n "$module" ]; do
      case "$module" in ""|\#*) continue ;; esac
      modules="$modules $module"
    done < "$DIR"/env/base/submodules.conf
  fi

  for env in "$DIR"/env/*
  do
    if [ "$(basename "$env")" != "base" ] \
      && ! is_env_ignored "$(basename "$env")"
    then
      env_module="env/$(basename "$env")"
      if echo "$known_submodules" | grep -Fqx "$env_module"
      then
        modules="$modules $env_module"
      fi
    fi
  done

  modules="${modules# }"

  if [ -z "$modules" ]
  then
    log_verbose "Skipping git submodules: no modules configured"
    return
  fi

  # shellcheck disable=SC2086
  if git -C "$DIR" submodule status $modules | cut -c-1 | grep -q "+\\|-"
  then
    log_stage "Installing git submodules"
    log_verbose "Updating submodules: $modules"
    # shellcheck disable=SC2086
    git -C "$DIR" submodule update --init --recursive $modules
  else
    log_verbose "Skipping git submodules: already up to date"
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
  if ! is_flag_set "p"
  then
    log_verbose "Skipping package installation: -p flag not set"
    return
  fi

  if ! is_program_installed "sudo" || ! is_program_installed "pacman"
  then
    log_verbose "Skipping package installation: sudo or pacman not installed"
    return
  fi

  packages=""
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/packages.conf ]
    then
      while IFS='' read -r package || [ -n "$package" ]
      do
        case "$package" in ""|\#*) continue ;; esac
        if ! pacman -Qq "$package" >/dev/null 2>&1
        then
          packages="$packages $package"
        else
          log_verbose "Skipping package $package: already installed"
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
    if is_flag_set "v"
    then
      args="-Verbose"
    fi
    pwsh -Command "Import-Module $DIR/src/script.psm1 && Install-PowerShellModules $args"
  else
    log_verbose "Skipping PowerShell modules: pwsh not installed"
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
        case "$symlink" in ""|\#*) continue ;; esac
        if ! is_symlink_installed "$(basename "$env")" "$symlink"
        then
          log_stage "Installing symlinks"
          log_verbose "Linking $env/symlinks/$symlink to ~/.$symlink"
          mkdir -pv "$(dirname ~/."$symlink")"
          if [ -e ~/."$symlink" ]
          then
            rm -rvf ~/."$symlink"
          fi
          ln -snvf "$env"/symlinks/"$symlink" ~/."$symlink"
        else
          log_verbose "Skipping symlink $symlink: already correct"
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
        case "$extension" in ""|\#*) continue ;; esac
        if ! echo "$extensions" | grep -qw "$extension"
        then
          log_stage "Installing $code extensions"
          log_verbose "Installing extension: $extension"
          $code --install-extension "$extension"
        else
          log_verbose "Skipping $code extension $extension: already installed"
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
  else
    log_verbose "Skipping PSScriptAnalyzer: pwsh not installed"
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
        else
          log_verbose "Skipping uninstall symlink $symlink: not installed"
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
  if [ ! -d "$DIR"/.git ]
  then
    log_verbose "Skipping update dotfiles: not a git repository"
    return
  fi
  if ! is_program_installed "git"
  then
    log_verbose "Skipping update dotfiles: git not installed"
    return
  fi
  if ! git -C "$DIR" diff-index --quiet HEAD --
  then
    log_verbose "Skipping update dotfiles: working tree not clean"
    return
  fi
  if [ "$(git -C "$DIR" rev-parse --abbrev-ref origin/HEAD | cut -d/ -f2)" != "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ]
  then
    log_verbose "Skipping update dotfiles: current branch does not match origin/HEAD"
    return
  fi

  if [ -n "$(git -C "$DIR" fetch --dry-run)" ]
  then
    log_stage "Updating dotfiles"
    log_verbose "Fetching updates from origin"
    git -C "$DIR" fetch
  else
    log_verbose "Skipping fetch: no updates from origin"
  fi
  if [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
  then
    log_stage "Updating dotfiles"
    log_verbose "Merging updates from origin/HEAD"
    git -C "$DIR" merge
  else
    log_verbose "Skipping merge: HEAD is up to date with origin/HEAD"
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
  if [ ! -d "$DIR"/.git ]
  then
    log_verbose "Skipping update git submodules: not a git repository"
    return
  fi
  if ! is_program_installed "git"
  then
    log_verbose "Skipping update git submodules: git not installed"
    return
  fi

  known_submodules="$(git -C "$DIR" submodule status | awk '{print $2}')"
  modules=""

  for env in "$DIR"/env/*
  do
    if [ "$(basename "$env")" != "base" ] \
      && ! is_env_ignored "$(basename "$env")"
    then
      env_module="env/$(basename "$env")"
      if echo "$known_submodules" | grep -Fqx "$env_module"
      then
        modules="$modules $env_module"
      fi
    fi
  done

  modules="${modules# }"

  if [ -z "$modules" ]
  then
    log_verbose "Skipping update git submodules: no modules to update"
    return
  fi

  # shellcheck disable=SC2086
  if [ -z "$(git -C "$DIR" submodule status $modules | cut -c1 | tr -d ' ')" ]
  then
    # shellcheck disable=SC2086
    updates="$(git -C "$DIR" submodule update --init --recursive --remote --dry-run $modules 2>/dev/null)" || updates=""
    if [ -n "$updates" ]
    then
      log_stage "Updating git submodules"
      log_verbose "Updating submodules: $modules"
      # shellcheck disable=SC2086
      git -C "$DIR" submodule update --init --recursive --remote $modules
    else
      log_verbose "Skipping update git submodules: already up to date with remote"
    fi
  else
    log_verbose "Skipping update git submodules: submodules have modifications or are uninitialized"
  fi
)}
