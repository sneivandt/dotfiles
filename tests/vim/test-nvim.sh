#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-nvim.sh — Neovim isolated configuration, startup, and plugin tests.
# Usage: sh tests/vim/test-nvim.sh [opens] [plugins] [configuration]
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"

test_configuration()
{(
  log_stage "Testing isolated Neovim configuration"
  python3 -B -m unittest discover -s "$DIR/tests/vim" -p 'test_*.py'
)}

test_opens()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  log_stage "Testing nvim startup"

  DIR="$DIR" timeout 10 nvim --headless -u NONE -i NONE -n \
    -c 'lua local ok, err = pcall(dofile, vim.env.DIR .. "/tests/vim/test-nvim-config.lua"); if not ok then vim.api.nvim_err_writeln(tostring(err)); vim.cmd("cquit") end' \
    -c 'qa!' </dev/null

  nvim --version >/dev/null 2>&1 || { printf "%sERROR: nvim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim binary OK"

  [ -d "$HOME/.config/nvim" ] || { printf "%sERROR: nvim config not installed: %s%s\n" "${RED}" "$HOME/.config/nvim" "${NC}" >&2; return 1; }
  timeout 120 nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim failed to load config%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim loads custom config"
)}

test_plugins()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  [ -f "$HOME/.config/nvim/nvimrc" ] || { printf "%sERROR: nvimrc not installed: %s%s\n" "${RED}" "$HOME/.config/nvim/nvimrc" "${NC}" >&2; return 1; }
  log_stage "Testing nvim plugins"

  lazy_dir="$HOME/.local/share/nvim/lazy"
  timeout 180 nvim --headless '+Lazy! restore' '+qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: lazy.nvim failed to restore locked plugins%s\n" "${RED}" "${NC}" >&2; return 1; }
  [ -d "$lazy_dir/lazy.nvim" ] || { printf "%sERROR: lazy.nvim not bootstrapped: %s%s\n" "${RED}" "$lazy_dir/lazy.nvim" "${NC}" >&2; return 1; }

  DIR="$DIR" timeout 30 nvim --headless \
    -c 'lua local ok, err = pcall(dofile, vim.env.DIR .. "/tests/vim/test-nvim-lock.lua"); if not ok then vim.api.nvim_err_writeln(tostring(err)); vim.cmd("cquit") end' \
    -c 'qa!' </dev/null || return 1

  count=$(find "$lazy_dir" -mindepth 1 -maxdepth 1 -type d | wc -l)
  log_verbose "Found $count plugin directories"

  timeout 30 nvim --headless +'qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim plugin load failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim starts with plugins OK"
)}

case "$0" in
  *test-nvim.sh)
    if [ $# -eq 0 ]; then
      set -- opens plugins configuration
    fi
    _lock_snapshot=$(mktemp)
    trap 'rm -f "$_lock_snapshot"' EXIT
    cp "$DIR/symlinks/vim/lazy-lock.json" "$_lock_snapshot"
    for _t in "$@"; do
      "test_${_t}"
      cmp "$DIR/symlinks/vim/lazy-lock.json" "$_lock_snapshot" || {
        log_error "Neovim tests changed the tracked plugin lockfile"
      }
    done
    ;;
esac
