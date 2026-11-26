#!/bin/sh
# Shared interactive startup helpers for Bash and Zsh.
# Keep POSIX-compatible and safe under `set -u` without altering caller options.

# Usage: _dotfiles_init_history <shell> <display-name>
_dotfiles_init_history() {
  _dih_shell="$1"
  _dih_label="$2"
  _dih_legacy="${XDG_CACHE_HOME:-$HOME/.cache}/$_dih_shell/history"
  HISTFILE="${XDG_STATE_HOME:-$HOME/.local/state}/$_dih_shell/history"
  _dih_dir=$(dirname -- "$HISTFILE")
  if [ -d "$_dih_dir" ] || mkdir -p "$_dih_dir"; then
    # Seed persistent state once. Keep the old file for shells still using it.
    if [ ! -e "$HISTFILE" ] && [ -f "$_dih_legacy" ]; then
      if ! (
        umask 077
        stage="$HISTFILE.migrate.$$"
        mkdir -- "$stage" || exit 1
        trap 'rm -f -- "$stage/history"; rmdir -- "$stage"' EXIT
        cp -- "$_dih_legacy" "$stage/history" &&
          { ln -T -- "$stage/history" "$HISTFILE" || [ -f "$HISTFILE" ]; }
      ); then
        printf 'Could not migrate %s history; keeping the previous location.\n' "$_dih_label" >&2
        HISTFILE="$_dih_legacy"
      fi
    fi
  else
    HISTFILE="$_dih_legacy"
  fi
  unset _dih_shell _dih_label _dih_legacy _dih_dir
}

# Prefer the lazy loader: it installs stub functions and only sources the real
# script (a ~200-500 ms Python startup) on first use.
_dotfiles_init_virtualenvwrapper() {
  if command -v virtualenvwrapper_lazy.sh >/dev/null 2>&1; then
    export WORKON_HOME="${WORKON_HOME:-$HOME/.venv}"
    # shellcheck source=/dev/null
    . virtualenvwrapper_lazy.sh
  elif command -v virtualenvwrapper.sh >/dev/null 2>&1; then
    export WORKON_HOME="${WORKON_HOME:-$HOME/.venv}"
    # shellcheck source=/dev/null
    . virtualenvwrapper.sh
  fi
}

# Prefer packaged scripts, including older Debian/Ubuntu FZF versions without
# --bash/--zsh.
# Usage: _dotfiles_init_fzf <bash|zsh>
_dotfiles_init_fzf() {
  _dif_shell="$1"
  _dif_found=0
  for _dif_dir in /usr/share/fzf /usr/share/doc/fzf/examples; do
    if [ -f "$_dif_dir/key-bindings.$_dif_shell" ] || [ -f "$_dif_dir/completion.$_dif_shell" ]; then
      # shellcheck source=/dev/null
      [ ! -f "$_dif_dir/key-bindings.$_dif_shell" ] || . "$_dif_dir/key-bindings.$_dif_shell"
      # shellcheck source=/dev/null
      [ ! -f "$_dif_dir/completion.$_dif_shell" ] || . "$_dif_dir/completion.$_dif_shell"
      _dif_found=1
      break
    fi
  done
  if [ "$_dif_found" -eq 0 ] && command -v fzf >/dev/null 2>&1; then
    eval "$(fzf "--$_dif_shell")"
  fi
  unset _dif_shell _dif_found _dif_dir
}
