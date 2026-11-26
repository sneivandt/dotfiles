#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-vim.sh — Vim isolated configuration and startup tests.
# Usage: sh tests/vim/test-vim.sh [opens] [configuration]
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"

test_configuration()
{(
  log_stage "Testing isolated Vim configuration"
  python3 -B -m unittest discover -s "$DIR/tests/vim" -p 'test_*.py'
)}

test_opens()
{(
  is_program_installed "vim" || { log_verbose "Skipping: vim not installed"; return 0; }
  log_stage "Testing vim startup"

  vim --version >/dev/null 2>&1 || { printf "%sERROR: vim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Vim binary OK"

  [ -f "$HOME/.vim/vimrc" ] || { printf "%sERROR: vimrc not installed: %s%s\n" "${RED}" "$HOME/.vim/vimrc" "${NC}" >&2; return 1; }
  timeout 5 vim -E -s -c 'quit' </dev/null >/dev/null 2>&1 || { printf "%sERROR: vim failed to load vimrc%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Vim loads custom vimrc"
)}

case "$0" in
  *test-vim.sh)
    if [ $# -eq 0 ]; then
      set -- opens configuration
    fi
    for _t in "$@"; do
      "test_${_t}"
    done
    ;;
esac
