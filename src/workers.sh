#!/bin/sh
set -o errexit
set -o nounset

. "$DIR"/src/helpers.sh
. "$DIR"/src/messages.sh

# configure_file_mode_bits
#
# Configure file mode bits.
configure_file_mode_bits()
{(
  for env in "$DIR"/env/*
  do
    if ! is_env_ignored "$(basename "$env")" \
      && [ -e "$env"/chmod.conf ]
    then
      while IFS='' read -r line || [ -n "$line" ]
      do
        chmod -c -R "$(echo "$line" | cut -d" " -f1)" ~/."$(echo "$line" | cut -d" " -f2)"
      done < "$env"/chmod.conf
    fi
  done
)}

# configure_fonts
#
# Configure fonts.
configure_fonts()
{(
  if ! is_env_ignored "arch-gui" \
    && is_program_installed "fc-list" \
    && is_program_installed "fc-cache" \
    && [ "$(fc-list : family | grep -f "$DIR"/env/arch-gui/fonts.conf -cx)" != "$(grep -c "" "$DIR"/env/arch-gui/fonts.conf | cut -d" " -f1)" ]
  then
    message_worker "Updating fonts"
    fc-cache
  fi
)}

# configure_shell
#
# Set the user shell.
configure_shell()
{(
  if is_program_installed "zsh" \
    && [ "$SHELL" != "$(zsh -c "command -vp zsh")" ] \
    && [ ! -f /.dockerenv ] \
    && [ "$(passwd --status "$USER" | cut -d" " -f2)" = "P" ]
  then
    message_worker "Configuring user shell"
    chsh -s "$(zsh -c "command -vp zsh")"
  fi
)}

# configure_systemd
#
# Configure systemd.
configure_systemd()
{(
  if [ "$(ps -p 1 -o comm=)" = "systemd" ] \
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
            message_worker "Configuring systemd"
            systemctl --user enable "$unit"
            if [ "$(systemctl is-system-running)" = "running" ]
            then
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
# Install dotfiles cli.
install_dotfiles_cli()
{(
  if [ "$(readlink -f "$DIR"/dotfiles.sh)" != "$(readlink -f ~/bin/dotfiles)" ]
  then
    message_worker "Installing dotfiles cli"
    mkdir -pv ~/bin
    ln -snvf "$DIR"/dotfiles.sh ~/bin/dotfiles
  fi
)}

# install_git_submodules
#
# Install git submodules.
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
    if eval "git -C $DIR submodule status $modules" | cut -c-1 | grep -q "+\\|-"
    then
      message_worker "Installing git submodules"
      eval "git -C $DIR submodule update --init --recursive $modules"
    fi
  fi
)}

# install_packages
#
# Install packages.
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
      message_worker "Installing packages"
      eval "sudo pacman -S --quiet --needed $packages"
    fi
  fi
)}

# install_symlinks
#
# Install symlinks.
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
          message_worker "Installing symlinks"
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
# Install vscode extensions.
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
          message_worker "Installing $code extensions"
          $code --install-extension "$extension"
        fi
      done < "$DIR/env/base-gui/vscode-extensions.conf"
    fi
  done
)}

# test_shellcheck
#
# run shellcheck.
test_shellcheck()
{(
  if ! is_program_installed "shellcheck"
  then
    message_error "shellcheck not installed"
  else
    message_worker "Verifying shell scripts"
    scripts="$DIR"/dotfiles.sh
    for script in "$DIR"/src/*
    do
      scripts="$scripts $script"
    done
    for env in "$DIR"/env/*
    do
      if ! is_env_ignored "$(basename "$env")" \
        && [ -e "$env"/symlinks.conf ]
      then
        while IFS='' read -r symlink || [ -n "$symlink" ]
        do
          if [ -d "$env/symlinks/$symlink" ]
          then
            tmpfile="$(mktemp)"
            find "$env/symlinks/$symlink" -type f > "$tmpfile"
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
          elif is_shell_script "$env/symlinks/$symlink"
          then
            scripts="$scripts $env/symlinks/$symlink"
          fi
        done < "$env"/symlinks.conf
      fi
    done
    eval "shellcheck $scripts"
  fi
)}

# uninstall_symlinks
#
# Uninstall symlinks.
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
          message_worker "Uninstalling symlinks"
          rm -vf ~/."$symlink"
        fi
      done < "$env"/symlinks.conf
    fi
  done
)}

# update_dotfiles
#
# Update dotfiles.
update_dotfiles()
{(
  if [ -d "$DIR"/.git ] \
    && is_program_installed "git" \
    && git -C "$DIR" diff-index --quiet HEAD -- \
    && [ "$(git -C "$DIR" remote show origin | sed -n -e "s/.*HEAD branch: //p")" = "$(git -C "$DIR" rev-parse --abbrev-ref HEAD)" ] \
    && [ "$(git -C "$DIR" log --format=format:%H -n 1 origin/HEAD)" != "$(git -C "$DIR" log --format=format:%H -n 1 HEAD)" ]
  then
    message_worker "Updating dotfiles"
    git -C "$DIR" pull
  fi
)}

# update_git_submodules
#
# Update git submodules.
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
    if [ -z "$(eval "git -C $DIR submodule status $modules | cut -c1")" ]
    then
      message_worker "Updating git submodules"
      eval "git -C $DIR submodule update --init --recursive --remote $modules"
    fi
  fi
)}