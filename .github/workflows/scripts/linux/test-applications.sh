#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-applications.sh — Application-level tests for git, zsh, vim, nvim.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
# When sourced with `.`, use BASH_SOURCE if available (bash); otherwise use pwd
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  # Fallback: assume we're already in the scripts directory or use relative path
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Zsh
# ---------------------------------------------------------------------------

test_zsh_completion()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  log_stage "Validating zsh completion"

  completion="$DIR/symlinks/config/zsh/completions/_dotfiles"
  [ -f "$completion" ] || { printf "%sERROR: completion file missing: %s%s\n" "${RED}" "$completion" "${NC}" >&2; return 1; }

  zsh -c "source '$completion'" >/dev/null 2>&1 || { printf "%sERROR: completion failed to load%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion file loads OK"

  zsh -c "source '$completion' && typeset -f _dotfiles >/dev/null" 2>&1 || { printf "%sERROR: _dotfiles not defined%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion functions defined"

  # Check profile count matches profiles.ini
  expected=$(list_available_profiles | wc -l)
  loaded=$(zsh -c "
    count=0
    while IFS= read -r l; do [[ \$l =~ '^\[([^]]+)\]\$' ]] && count=\$((count+1)); done < '$DIR/conf/profiles.ini'
    echo \$count
  ")
  [ "$loaded" -eq "$expected" ] || { printf "%sERROR: profile count mismatch: %d vs %d%s\n" "${RED}" "$loaded" "$expected" "${NC}" >&2; return 1; }
  log_verbose "Loaded $loaded profiles"
)}

# ---------------------------------------------------------------------------
# Vim
# ---------------------------------------------------------------------------

test_vim_opens()
{(
  is_program_installed "vim" || { log_verbose "Skipping: vim not installed"; return 0; }
  log_stage "Testing vim startup"

  vim --version >/dev/null 2>&1 || { printf "%sERROR: vim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Vim binary OK"

  if [ -f "$HOME/.vim/vimrc" ]; then
    timeout 5 vim -E -s -c 'quit' </dev/null >/dev/null 2>&1 || { printf "%sERROR: vim failed to load vimrc%s\n" "${RED}" "${NC}" >&2; return 1; }
    log_verbose "Vim loads custom vimrc"
  fi
)}

# ---------------------------------------------------------------------------
# Neovim
# ---------------------------------------------------------------------------

test_nvim_opens()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  log_stage "Testing nvim startup"

  nvim --version >/dev/null 2>&1 || { printf "%sERROR: nvim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim binary OK"

  if [ -d "$HOME/.config/nvim" ]; then
    timeout 30 nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim failed to load config%s\n" "${RED}" "${NC}" >&2; return 1; }
    log_verbose "Nvim loads custom config"
  fi
)}

test_nvim_plugins()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  [ -f "$HOME/.config/nvim/nvimrc" ] || { log_verbose "Skipping: nvimrc not installed"; return 0; }
  log_stage "Testing nvim plugins"

  lazy_dir="$HOME/.local/share/nvim/lazy"
  [ -d "$lazy_dir/lazy.nvim" ] || { log_verbose "lazy.nvim not bootstrapped yet"; return 0; }

  count=$(find "$lazy_dir" -mindepth 1 -maxdepth 1 -type d | wc -l)
  log_verbose "Found $count plugin directories"

  timeout 30 nvim --headless +'qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim plugin load failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim starts with plugins OK"
)}

# ---------------------------------------------------------------------------
# Git
# ---------------------------------------------------------------------------

test_git_config()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git configuration"

  git --version >/dev/null 2>&1 || { printf "%sERROR: git --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  [ -f "$HOME/.config/git/config" ] || { log_verbose "Custom git config not installed"; return 0; }
  log_verbose "Custom git config found"

  # Check key config values
  for kv in "init.defaultBranch=main" "pull.rebase=true" "merge.conflictstyle=zdiff3" "push.autoSetupRemote=true" "diff.algorithm=histogram"; do
    key="${kv%%=*}"; expected="${kv#*=}"
    actual="$(git config --get "$key" 2>/dev/null || echo "")"
    if [ "$actual" = "$expected" ]; then
      log_verbose "✓ $key = $actual"
    else
      printf "%sWARNING: %s expected '%s', got '%s'%s\n" "${YELLOW}" "$key" "$expected" "$actual" "${NC}" >&2
    fi
  done
)}

test_git_aliases()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git aliases"
  [ -f "$HOME/.config/git/config" ] || { log_verbose "Git config not installed"; return 0; }
  [ -f "$HOME/.config/git/aliases" ] || { log_verbose "Aliases file not installed"; return 0; }

  for a in st br lo ci; do
    if git config --get "alias.$a" >/dev/null 2>&1; then
      log_verbose "✓ alias.$a = $(git config --get "alias.$a")"
    else
      printf "%sWARNING: alias.%s not defined%s\n" "${YELLOW}" "$a" "${NC}" >&2
    fi
  done
)}

test_git_behavior()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git behavior"
  [ -f "$HOME/.config/git/config" ] || { log_verbose "Git config not installed"; return 0; }

  repo="$(mktemp -d)"
  trap 'rm -rf "$repo"' EXIT
  git init "$repo" >/dev/null 2>&1
  cd "$repo"
  git config user.name "CI Test"
  git config user.email "ci@test.local"

  # Default branch should be 'main'
  branch="$(git branch --show-current)"
  if [ "$branch" = "main" ]; then
    log_verbose "✓ Default branch is main"
  else
    printf "%sWARNING: default branch is '%s'%s\n" "${YELLOW}" "$branch" "${NC}" >&2
  fi

  # Can create a commit
  echo test > test.txt && git add test.txt
  git commit -m "Test commit" >/dev/null 2>&1 || { printf "%sWARNING: commit failed%s\n" "${YELLOW}" "${NC}" >&2; return 0; }
  log_verbose "✓ Commit created successfully"
)}

# Execute tests when run directly: sh test-applications.sh <app> <test1> [test2...]
case "$0" in
  *test-applications.sh)
    if [ $# -ge 2 ]; then
      _app="$1"; shift
      for _t in "$@"; do
        "test_${_app}_${_t}"
      done
    fi
    ;;
esac
