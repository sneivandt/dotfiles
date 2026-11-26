#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-zsh.sh — Zsh configuration, completion, and history widget tests.
# Usage: sh tests/shell/test-zsh.sh [completion] [history] [configuration]
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"

test_configuration()
{(
  log_stage "Testing isolated shell configuration"
  python3 -B "$DIR/tests/shell/test-shell-config.py"
)}

test_completion()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  log_stage "Validating zsh completion"

  completion="$DIR/symlinks/config/zsh/completions/_dotfiles"
  [ -f "$completion" ] || { printf "%sERROR: completion file missing: %s%s\n" "${RED}" "$completion" "${NC}" >&2; return 1; }

  # Runtime completion resolves `dotfiles` when the registration script loads.
  PATH="$DIR/bin:$PATH" zsh -c "autoload -Uz compinit; compinit -u; source '$completion'" >/dev/null 2>&1 || { printf "%sERROR: completion failed to load%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion file loads OK"

  PATH="$DIR/bin:$PATH" zsh -c "autoload -Uz compinit; compinit -u; source '$completion' && typeset -f _clap_dynamic_completer_dotfiles >/dev/null" 2>&1 || { printf "%sERROR: runtime dotfiles completer not defined%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion functions defined"
)}

test_history()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  is_program_installed "fzf" || { log_verbose "Skipping: fzf not installed"; return 0; }
  log_stage "Testing zsh history selection"

  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/.config"
  ln -s "$DIR/symlinks/config/shell" "$fixture/.config/shell"
  HOME="$fixture" XDG_CACHE_HOME="$fixture/.cache" DIR="$DIR" \
    zsh -d -f -i -c '
      source "$DIR/symlinks/zshrc"
      [[ "$(bindkey -M emacs "^R")" == "\"^R\" fzf-history-widget" ]] || exit 1
      (( ! $+widgets[fzf-history-widget-accept] )) || exit 1
    '
  log_verbose "Ctrl+R uses the standard non-executing fzf history widget"
)}

case "$0" in
  *test-zsh.sh)
    if [ $# -eq 0 ]; then
      set -- completion history configuration
    fi
    for _t in "$@"; do
      "test_${_t}"
    done
    ;;
esac
